import { create } from "zustand";
import { persist, type PersistStorage, type StorageValue } from "zustand/middleware";

import { nanoid } from "nanoid";
import equal from "fast-deep-equal";
import { localForageStorage } from "@/lib/localforage-storage";
import { listCanvasProjects, saveCanvasProject, syncCanvasProjects } from "@/services/api/canvas-tasks";
import { fetchUserConfig } from "@/services/api/user-config";
import { useUserStore } from "@/stores/use-user-store";
import { isDesktopRuntime, loadDesktopCanvasProjects, loadDesktopCanvasDeletedIds, saveDesktopCanvasProject, restoreDesktopCanvasVersion } from "@/services/desktop-runtime";
import type { CanvasBackgroundMode } from "@/lib/canvas-theme";
import { validateCanvasGraph } from "../utils/canvas-graph";
import type { CanvasAgentConfig, CanvasAssistantSession, CanvasConnection, CanvasNodeData, CanvasPendingAgentRequest, ViewportTransform } from "../types";
import {
    CANVAS_OPERATION_PROTOCOL_VERSION,
    applyCanvasOperationBatch,
    buildCanvasStructureOperations,
    createCanvasOperationState,
    migrateCanvasProject,
    rebindCanvasProjectIdentity,
    type CanvasOperationBatch,
    type CanvasOperationOutcome,
    type CanvasOperationState,
} from "../protocol/canvas-operation-protocol";

export type CanvasSidePanelState = {
    open: boolean;
    width: number;
};

export const DEFAULT_CANVAS_SIDE_PANEL: CanvasSidePanelState = { open: true, width: 320 };
export const DEFAULT_CANVAS_AGENT_PANEL: CanvasSidePanelState = { open: false, width: 390 };

export type CanvasProject = {
    __desktopRevision?: string;
    quarantinedConnections?: Array<{ connection: CanvasConnection; reason: string }>;
    id: string;
    title: string;
    createdAt: string;
    updatedAt: string;
    nodes: CanvasNodeData[];
    connections: CanvasConnection[];
    chatSessions: CanvasAssistantSession[];
    activeChatId: string | null;
    agentConfig: CanvasAgentConfig | null;
    autoTitlePending: boolean;
    pendingAgentRequest?: CanvasPendingAgentRequest;
    backgroundMode: CanvasBackgroundMode;
    showImageInfo: boolean;
    viewport: ViewportTransform;
    sidePanel: CanvasSidePanelState;
    agentPanel: CanvasSidePanelState;
    operationState: CanvasOperationState;
};

type CanvasStore = {
    hydrated: boolean;
    desktopPersistenceStatus: "not_applicable" | "checking" | "database" | "error";
    desktopPersistenceError: string | null;
    projects: CanvasProject[];
    restoredRevisions: Record<string, string>;
    saveStatus: Record<string, { state: "pending" | "saved" | "error"; error?: string }>;
    retrySave: (id: string) => Promise<void>;
    restoreVersion: (id: string, sequence: number, expectedRevision?: string) => Promise<void>;
    createProject: (title?: string, options?: { agentConfig?: CanvasAgentConfig; pendingAgentRequest?: CanvasPendingAgentRequest }) => string;
    importProject: (project: Partial<CanvasProject>) => string;
    openProject: (id: string) => CanvasProject | null;
    renameProject: (id: string, title: string) => void;
    deleteProjects: (ids: string[]) => void;
    updateProject: (id: string, patch: Partial<Pick<CanvasProject, "nodes" | "connections" | "chatSessions" | "activeChatId" | "agentConfig" | "autoTitlePending" | "backgroundMode" | "showImageInfo" | "viewport" | "sidePanel" | "agentPanel" | "pendingAgentRequest">>) => void;
    applyOperationBatch: (batch: CanvasOperationBatch) => CanvasOperationOutcome<CanvasProject> | null;
    refreshFromDesktop: (projectId?: string) => Promise<void>;
    syncWithRemote: (token: string, syncEnabled: boolean) => Promise<void>;
    setSyncEnabled: (enabled: boolean) => void;
};

const initialViewport: ViewportTransform = { x: 0, y: 0, k: 1 };
const CANVAS_STORE_KEY = "infinite-canvas:canvas_store";
const CANVAS_STORE_INDEX_KEY = "infinite-canvas:canvas_store:index";
const CANVAS_PROJECT_PREFIX = "infinite-canvas:canvas_project:";
const UI_ONLY_PROJECT_KEYS = new Set(["viewport", "sidePanel", "agentPanel"]);
type PersistedCanvasState = Pick<CanvasStore, "projects">;
type CanvasStoreIndex = { version: 1; ids: string[] };
let saveTimer: ReturnType<typeof setTimeout> | null = null;
let queuedPersistState: PersistedCanvasState | null = null;
let accountCanvasSyncEnabled = false;
const projectSaveTimers = new Map<string, ReturnType<typeof setTimeout>>();
const lastWrittenProjects = new Map<string, CanvasProject>();
let canvasShardsReady = false;

const pendingProjects = new Map<string, CanvasProject>();
const saveChains = new Map<string, Promise<void>>();
const RECOVERY_INDEX = "infinite-canvas:recovery:index";
const RECOVERY_PREFIX = "infinite-canvas:recovery:project:";
let recoveryChain: Promise<void> = Promise.resolve();
let localPersistChain: Promise<void> = Promise.resolve();
const deletedDesktopIds = new Set<string>();
const restoringProjects = new Set<string>();

function saveStatus(id: string, state: "pending" | "saved" | "error", error?: string) {
    useCanvasStore.setState((store) => ({ saveStatus: { ...store.saveStatus, [id]: { state, error } } }));
}

function checkpointPending() {
    recoveryChain = recoveryChain.catch(() => undefined).then(async () => {
        const pending = [...pendingProjects.values()];
        for (const project of pending) await localForageStorage.setItem(RECOVERY_PREFIX + project.id, JSON.stringify(project));
        await localForageStorage.setItem(RECOVERY_INDEX, JSON.stringify(pending.map((project) => project.id)));
    });
    return recoveryChain;
}

async function loadRecoveryProjects() {
    const raw = await localForageStorage.getItem(RECOVERY_INDEX);
    const ids: string[] = raw ? JSON.parse(raw) : [];
    for (const id of ids) {
        const value = await localForageStorage.getItem(RECOVERY_PREFIX + id);
        if (value) pendingProjects.set(id, JSON.parse(value));
    }
}

function flushProject(id: string): Promise<void> {
    const task = (saveChains.get(id) || Promise.resolve()).catch(() => undefined).then(async () => {
      if (restoringProjects.has(id)) return;
      while (pendingProjects.has(id)) {
        const project = pendingProjects.get(id);
        if (!project) return;
        try {
            await checkpointPending();
            validateCanvasGraph(project);
            if (deletedDesktopIds.has(id)) throw new Error("画布已在桌面删除；未保存内容仍留在恢复记录中");
            let saved: CanvasProject;
            if (isDesktopRuntime()) saved = await saveDesktopCanvasProject(project);
            else {
                const token = useUserStore.getState().token;
                if (!token || !accountCanvasSyncEnabled) return;
                saved = await saveCanvasProject(token, project);
            }
            const { __desktopRevision: savedRevision, ...savedContent } = saved;
            const { __desktopRevision: _expectedRevision, ...sentContent } = project;
            if (!equal(JSON.parse(JSON.stringify(savedContent)), JSON.parse(JSON.stringify(sentContent)))) {
                throw new Error("桌面已有较新的修改，当前编辑已保留；请先核对两个版本");
            }
            if (pendingProjects.get(id) === project) {
                pendingProjects.delete(id);
            } else if (pendingProjects.has(id)) {
                const latest = { ...pendingProjects.get(id)!, __desktopRevision: savedRevision };
                pendingProjects.set(id, latest);
            }
            useCanvasStore.setState((state) => ({ projects: state.projects.map((current) => current.id === id ? { ...current, __desktopRevision: savedRevision } : current) }));
            await checkpointPending();
            if (!pendingProjects.has(id)) saveStatus(id, "saved");
        } catch (error) {
            saveStatus(id, "error", error instanceof Error ? error.message : String(error));
            throw error;
        }
      }
    });
    saveChains.set(id, task);
    return task;
}

function queueProjectSave(project: CanvasProject) {
    saveStatus(project.id, "pending");
    if (!isDesktopRuntime() && (!useUserStore.getState().token || !accountCanvasSyncEnabled)) return;
    pendingProjects.set(project.id, project);
    void checkpointPending().catch((error) => saveStatus(project.id, "error", String(error)));
    const previous = projectSaveTimers.get(project.id);
    if (previous) clearTimeout(previous);
    projectSaveTimers.set(project.id, setTimeout(() => {
        projectSaveTimers.delete(project.id);
        void flushProject(project.id).catch(() => undefined);
    }, 400));
}

async function readDesktopProjects(localProjects: CanvasProject[]) {
    const [desktopProjects, deletedIds] = await Promise.all([loadDesktopCanvasProjects<CanvasProject>(), loadDesktopCanvasDeletedIds()]);
    deletedDesktopIds.clear();
    deletedIds.forEach((id) => deletedDesktopIds.add(id));
    for (const project of localProjects.filter((item) => deletedDesktopIds.has(item.id))) {
        await localForageStorage.setItem("infinite-canvas:recovery:deleted:" + project.id, JSON.stringify(project));
    }
    // A completed write may have lost its reply. Only exact content equality can
    // acknowledge recovery; a changed timestamp or any other edit remains a conflict.
    let acknowledgedRecovery = false;
    for (const desktopProject of desktopProjects) {
        const pending = pendingProjects.get(desktopProject.id);
        if (!pending || deletedDesktopIds.has(desktopProject.id)) continue;
        const { __desktopRevision: _pendingRevision, ...pendingContent } = pending;
        const { __desktopRevision: _savedRevision, ...savedContent } = desktopProject;
        if (equal(JSON.parse(JSON.stringify(pendingContent)), JSON.parse(JSON.stringify(savedContent)))) {
            pendingProjects.delete(desktopProject.id);
            acknowledgedRecovery = true;
        }
    }
    if (acknowledgedRecovery) await checkpointPending();
    const desktopIds = new Set(desktopProjects.map((project) => project.id));
    for (const project of localProjects.filter((item) => !desktopIds.has(item.id) && !deletedDesktopIds.has(item.id))) {
        if (!pendingProjects.has(project.id)) pendingProjects.set(project.id, project);
    }
    useCanvasStore.setState({ desktopPersistenceStatus: "database", desktopPersistenceError: null });
    return mergeDesktopCanvasProjects(desktopProjects, localProjects.filter((project) => !deletedDesktopIds.has(project.id)));
}

function cancelProjectSaves(ids: string[]) {
    ids.forEach((id) => {
        const timer = projectSaveTimers.get(id);
        if (!timer) return;
        clearTimeout(timer);
        projectSaveTimers.delete(id);
    });
}

async function reconcileCanvasProjects(token: string, remoteProjects: CanvasProject[], localProjects: CanvasProject[]) {
    const remoteById = new Map(remoteProjects.map((project) => [project.id, project]));
    const missingProjects = localProjects.filter((project) => !remoteById.has(project.id));
    const existingLocalProjects = localProjects.filter((project) => remoteById.has(project.id));
    const projects = missingProjects.length
        ? await syncCanvasProjects(token, missingProjects)
              .then((syncedProjects) => mergeCanvasProjects(syncedProjects, existingLocalProjects))
              .catch(() => mergeCanvasProjects(remoteProjects, localProjects))
        : mergeCanvasProjects(remoteProjects, existingLocalProjects);

    localProjects.forEach((project) => {
        const remote = remoteById.get(project.id);
        if (remote && Date.parse(project.updatedAt || "") > Date.parse(remote.updatedAt || "")) {
            queueProjectSave(project);
        }
    });

    return projects;
}

function isUiOnlyProjectPatch(patch: object) {
    const keys = Object.keys(patch);
    return keys.length > 0 && keys.every((key) => UI_ONLY_PROJECT_KEYS.has(key));
}

function rememberWrittenProjects(projects: CanvasProject[]) {
    lastWrittenProjects.clear();
    projects.forEach((project) => lastWrittenProjects.set(project.id, project));
}

function projectNeedsWrite(project: CanvasProject) {
    const previous = lastWrittenProjects.get(project.id);
    if (!previous) return true;
    return (
        previous !== project &&
        (previous.updatedAt !== project.updatedAt ||
            previous.title !== project.title ||
            previous.nodes !== project.nodes ||
            previous.connections !== project.connections ||
            previous.chatSessions !== project.chatSessions ||
            previous.activeChatId !== project.activeChatId ||
            previous.agentConfig !== project.agentConfig ||
            previous.autoTitlePending !== project.autoTitlePending ||
            previous.backgroundMode !== project.backgroundMode ||
            previous.showImageInfo !== project.showImageInfo ||
            previous.pendingAgentRequest !== project.pendingAgentRequest ||
            previous.viewport !== project.viewport || previous.sidePanel !== project.sidePanel || previous.agentPanel !== project.agentPanel)
    );
}

async function loadLocalProjects(): Promise<CanvasProject[]> {
    const indexValue = await localForageStorage.getItem(CANVAS_STORE_INDEX_KEY);
    if (indexValue) {
        const index = JSON.parse(indexValue) as CanvasStoreIndex;
        if (index?.version === 1 && Array.isArray(index.ids)) {
            const projects = (
                await Promise.all(
                    index.ids.map(async (id) => {
                        const raw = await localForageStorage.getItem(CANVAS_PROJECT_PREFIX + id);
                        return raw ? (JSON.parse(raw) as CanvasProject) : null;
                    }),
                )
            ).filter((project): project is CanvasProject => Boolean(project));
            canvasShardsReady = true;
            return projects;
        }
    }
    canvasShardsReady = false;
    const legacy = await localForageStorage.getItem(CANVAS_STORE_KEY);
    if (!legacy) return [];
    const parsed = JSON.parse(legacy) as StorageValue<CanvasStore>;
    return (parsed.state as PersistedCanvasState)?.projects || [];
}

function persistLocalProjects(projects: CanvasProject[]) {
    localPersistChain = localPersistChain.catch(() => undefined).then(() => writeLocalProjects(projects));
    return localPersistChain;
}

async function writeLocalProjects(projects: CanvasProject[]) {
    projects.forEach(validateCanvasGraph);
    const dirty = canvasShardsReady ? projects.filter(projectNeedsWrite) : projects;
    const nextIds = new Set(projects.map((project) => project.id));
    const removedIds = Array.from(lastWrittenProjects.keys()).filter((id) => !nextIds.has(id));
    await Promise.all([
        ...dirty.map((project) =>
            localForageStorage.setItem(CANVAS_PROJECT_PREFIX + project.id, JSON.stringify(project)),
        ),
        ...removedIds.map((id) => localForageStorage.removeItem(CANVAS_PROJECT_PREFIX + id)),
        localForageStorage.setItem(
            CANVAS_STORE_INDEX_KEY,
            JSON.stringify({ version: 1, ids: projects.map((project) => project.id) } satisfies CanvasStoreIndex),
        ),
    ]);
    rememberWrittenProjects(projects);
    canvasShardsReady = true;
}

const canvasStorage: PersistStorage<CanvasStore> = {
    getItem: async (name) => {
        await loadRecoveryProjects();
        const localProjects = mergeCanvasProjects(await loadLocalProjects(), [...pendingProjects.values()]);
        const token = useUserStore.getState().token;
        const localParsed = {
            state: { projects: localProjects },
            version: 0,
        } as StorageValue<CanvasStore>;
        const localHasData = localProjects.length > 0;

        if (isDesktopRuntime()) {
            try {
                const projects = await readDesktopProjects(localProjects);
                if (projects.length > 0 || localParsed) {
                    const nextState = { projects };
                    const parsed = {
                        state: nextState,
                        version: 0,
                    } as StorageValue<CanvasStore>;
                    queuedPersistState = nextState;
                    await localForageStorage.setItem(
                        name,
                        JSON.stringify(parsed),
                    );
                    return parsed;
                }
            } catch (error) {
                console.error("Failed to hydrate desktop canvas projects", error);
            }
        }

        if (token) {
            try {
                const [userConfig, remoteProjects] = await Promise.all([fetchUserConfig(token), listCanvasProjects(token)]);
                accountCanvasSyncEnabled = userConfig.syncCapabilities?.userData === true;

                if (accountCanvasSyncEnabled && localHasData) {
                    const projects = await reconcileCanvasProjects(
                        token,
                        remoteProjects.map((project) => migrateCanvasProject(project)),
                        localProjects,
                    );

                    const nextState = { projects };
                    const parsed = {
                        state: nextState,
                        version: 0,
                    } as StorageValue<CanvasStore>;
                    queuedPersistState = nextState;
                    await persistLocalProjects(remoteProjects);
                    return parsed;
                }
            } catch (error) {
                console.error("Failed to hydrate canvas projects from remote", error);
            }
        }

        if (!localProjects.length) return null;
        queuedPersistState = localParsed.state as PersistedCanvasState;
        rememberWrittenProjects(localProjects);
        return localParsed;
    },

    setItem: (_name, value) => {
        const nextState = value.state as PersistedCanvasState;
        if (queuedPersistState && queuedPersistState.projects === nextState.projects) {
            return;
        }
        queuedPersistState = nextState;
        if (saveTimer) clearTimeout(saveTimer);
        saveTimer = setTimeout(() => {
            saveTimer = null;
            void persistLocalProjects(nextState.projects || []).then(() => {
                if (!isDesktopRuntime()) for (const project of nextState.projects || []) {
                    if (!pendingProjects.has(project.id) && useCanvasStore.getState().projects.find((p) => p.id === project.id) === project) saveStatus(project.id, "saved");
                }
            }).catch((error) => {
                for (const project of nextState.projects || []) saveStatus(project.id, "error", `本机保存失败：${String(error)}`);
            });
        }, 400);
    },
    removeItem: (name) => localForageStorage.removeItem(name),
};

export const useCanvasStore = create<CanvasStore>()(
    persist(
        (set, get) => ({
            hydrated: false,
            desktopPersistenceStatus: isDesktopRuntime() ? "checking" : "not_applicable",
            desktopPersistenceError: null,
            projects: [],
            saveStatus: {},
            restoredRevisions: {},
            retrySave: async (id) => {
                saveStatus(id, "pending");
                try {
                    await persistLocalProjects(get().projects);
                    await flushProject(id);
                    if (!pendingProjects.has(id)) saveStatus(id, "saved");
                } catch (error) {
                    saveStatus(id, "error", String(error));
                    throw error;
                }
            },
            restoreVersion: async (id, sequence, expectedRevision) => {
                if (!isDesktopRuntime()) throw new Error("版本恢复需要桌面版");
                if (restoringProjects.has(id)) throw new Error("这个画布正在恢复版本");
                await get().retrySave(id);
                const before = get().projects.find((project) => project.id === id);
                if (!before?.__desktopRevision || (expectedRevision && expectedRevision !== before.__desktopRevision)) throw new Error("画布已变化，请重新预览差异后恢复");
                if (before.pendingAgentRequest || before.nodes.some((node) => node.metadata?.status === "loading") || before.chatSessions.some((session) => session.messages.some((message) => message.status === "thinking" || message.status === "running" || (message.status === "waiting" && message.activity)))) throw new Error("请先停止当前画布正在运行的任务或对话，再恢复历史版本");
                restoringProjects.add(id);
                saveStatus(id, "pending");
                try {
                    const restored = await restoreDesktopCanvasVersion<CanvasProject>(id, sequence, before.__desktopRevision, crypto.randomUUID());
                    if (pendingProjects.has(id) || get().projects.find((project) => project.id === id) !== before) {
                        await checkpointPending();
                        throw new Error("历史版本已恢复，但恢复期间又有新编辑；新编辑已保留，请另存当前编辑后重新打开画布核对");
                    }
                    set((state) => ({ projects: state.projects.map((project) => project.id === id ? restored : project), restoredRevisions: { ...state.restoredRevisions, [id]: restored.__desktopRevision! } }));
                    await persistLocalProjects(get().projects);
                    if (!pendingProjects.has(id)) saveStatus(id, "saved");
                } catch (error) {
                    saveStatus(id, "error", String(error));
                    throw error;
                } finally {
                    restoringProjects.delete(id);
                }
            },
            createProject: (title = "未命名画布", options) => {
                const now = new Date().toISOString();
                const id = nanoid();
                const project: CanvasProject = {
                    id,
                    title,
                    createdAt: now,
                    updatedAt: now,
                    nodes: [],
                    connections: [],
                    chatSessions: [],
                    activeChatId: null,
                    agentConfig: options?.agentConfig || null,
                    autoTitlePending: true,
                    pendingAgentRequest: options?.pendingAgentRequest,
                    backgroundMode: "lines",
                    showImageInfo: false,
                    viewport: initialViewport,
                    sidePanel: DEFAULT_CANVAS_SIDE_PANEL,
                    agentPanel: options?.pendingAgentRequest ? { ...DEFAULT_CANVAS_AGENT_PANEL, open: true } : DEFAULT_CANVAS_AGENT_PANEL,
                    operationState: createCanvasOperationState({ nodes: [] }),
                };
                set((state) => ({
                    projects: [project, ...state.projects],
                }));
                queueProjectSave(project);
                return id;
            },
            importProject: (source) => {
                validateCanvasGraph({ nodes: source.nodes || [], connections: source.connections || [] });
                const now = new Date().toISOString();
                const id = nanoid();
                const project = rebindCanvasProjectIdentity(migrateCanvasProject<CanvasProject>({
                    ...source,
                    __desktopRevision: undefined,
                    id,
                    title: source.title || "导入画布",
                    createdAt: source.createdAt || now,
                    updatedAt: now,
                    nodes: source.nodes || [],
                    connections: source.connections || [],
                    chatSessions: source.chatSessions || [],
                    activeChatId: source.activeChatId || null,
                    agentConfig: source.agentConfig || null,
                    autoTitlePending: false,
                    backgroundMode: source.backgroundMode || "lines",
                    showImageInfo: source.showImageInfo || false,
                    viewport: source.viewport || initialViewport,
                    sidePanel: source.sidePanel || DEFAULT_CANVAS_SIDE_PANEL,
                    agentPanel: source.agentPanel || DEFAULT_CANVAS_AGENT_PANEL,
                    operationState: source.operationState || createCanvasOperationState({ nodes: source.nodes || [] }),
                }), id);
                set((state) => ({
                    projects: [project, ...state.projects],
                }));
                queueProjectSave(project);
                return project.id;
            },
            openProject: (id) => get().projects.find((item) => item.id === id) || null,
            renameProject: (id, title) => {
                const sourceProject = get().projects.find((item) => item.id === id);
                if (!sourceProject) return;
                const project = migrateCanvasProject(sourceProject);
                const nextTitle = title.trim() || project.title;
                if (nextTitle === project.title && !project.autoTitlePending) return;
                const timestamp = new Date().toISOString();
                const outcome = applyCanvasOperationBatch(project, {
                    protocolVersion: CANVAS_OPERATION_PROTOCOL_VERSION,
                    actor: "human",
                    requestId: `ui-title-${nanoid()}`,
                    projectId: id,
                    baseRevision: project.operationState.revision,
                    timestamp,
                    operations: [{ type: "project.update", title: nextTitle }],
                }, { now: () => timestamp });
                if (!outcome.result.ok) return;
                const nextProject = {
                    ...outcome.project,
                    autoTitlePending: false,
                };
                set((state) => ({
                    projects: state.projects.map((item) => (item.id === id ? nextProject : item)),
                }));
                queueProjectSave(nextProject);
            },
            deleteProjects: (ids) => {
                cancelProjectSaves(ids);
                set((state) => ({
                    projects: state.projects.filter((project) => !ids.includes(project.id)),
                }));
            },
            updateProject: (id, patch) => {
                const sourceProject = get().projects.find(
                    (item) => item.id === id,
                );
                if (!sourceProject) return;
                const project = migrateCanvasProject(sourceProject);
                const uiOnly = isUiOnlyProjectPatch(patch);
                const targetNodes = patch.nodes || project.nodes;
                const targetConnections = patch.connections || project.connections;
                const operations = buildCanvasStructureOperations(project, targetNodes, targetConnections);
                let nextProject: CanvasProject = project;
                if (operations.length) {
                    const timestamp = new Date().toISOString();
                    const outcome = applyCanvasOperationBatch(project, {
                        protocolVersion: CANVAS_OPERATION_PROTOCOL_VERSION,
                        actor: "human",
                        requestId: `ui-${nanoid()}`,
                        projectId: id,
                        baseRevision: project.operationState.revision,
                        timestamp,
                        operations,
                    }, { now: () => timestamp });
                    if (!outcome.result.ok) {
                        const draft = { ...project, ...patch, updatedAt: timestamp };
                        set((state) => ({ projects: state.projects.map((item) => item.id === id ? draft : item) }));
                        queueProjectSave(draft);
                        saveStatus(id, "error", outcome.result.error?.message || "画布修改未通过校验，草稿已保留");
                        return;
                    }
                    nextProject = outcome.project;
                }
                const { nodes: _nodes, connections: _connections, ...projectPatch } = patch;
                const projectPatchChanged = Object.entries(projectPatch).some(
                    ([key, value]) => JSON.stringify(project[key as keyof CanvasProject]) !== JSON.stringify(value),
                );
                if (!operations.length && !projectPatchChanged) return;
                nextProject = {
                    ...nextProject,
                    ...projectPatch,
                    updatedAt: operations.length ? nextProject.updatedAt : uiOnly ? project.updatedAt : new Date().toISOString(),
                };
                set((state) => ({
                    projects: state.projects.map((item) => (item.id === id ? nextProject : item)),
                }));
                if (!uiOnly) queueProjectSave(nextProject);
            },
            applyOperationBatch: (batch) => {
                const sourceProject = get().projects.find((project) => project.id === batch.projectId);
                if (!sourceProject) return null;
                const outcome = applyCanvasOperationBatch(migrateCanvasProject(sourceProject), batch);
                set((state) => ({
                    projects: state.projects.map((project) => project.id === batch.projectId ? outcome.project : project),
                }));
                queueProjectSave(outcome.project);
                return outcome;
            },
            syncWithRemote: async (token, syncEnabled) => {
                if (!useUserStore.getState().token) return;
                accountCanvasSyncEnabled = syncEnabled;
                if (!syncEnabled) return;
                const localProjects = get().projects.map((project) => migrateCanvasProject(project));
                const remoteProjects = await listCanvasProjects(token).catch(
                    () => null,
                );
                if (!remoteProjects) return;
                const projects = await reconcileCanvasProjects(
                    token,
                    remoteProjects.map((project) => migrateCanvasProject(project)),
                    localProjects,
                );
                if (saveTimer) {
                    clearTimeout(saveTimer);
                    saveTimer = null;
                }
                const nextState = { projects };
                queuedPersistState = nextState;
                set(nextState);
                await persistLocalProjects(projects);
            },
            setSyncEnabled: (enabled) => {
                accountCanvasSyncEnabled = enabled;
            },
            refreshFromDesktop: async () => {
                if (!isDesktopRuntime()) return;
                const projects = await readDesktopProjects(get().projects);
                queuedPersistState = { projects };
                set({ projects });
                await persistLocalProjects(projects);
                const failures = await Promise.allSettled([...pendingProjects.keys()].filter((id) => !deletedDesktopIds.has(id)).map(flushProject));
                const failed = failures.find((result) => result.status === "rejected");
                if (failed?.status === "rejected") throw failed.reason;
            },
        }),
        {
            name: CANVAS_STORE_KEY,
            storage: canvasStorage,
            partialize: (state) =>
                ({
                    projects: state.projects,
                }) as StorageValue<CanvasStore>["state"],
            onRehydrateStorage: () => () => {
                useCanvasStore.setState({ hydrated: true });
            },
        },
    ),
);

export function mergeCanvasProjects(remoteProjects: CanvasProject[], localProjects: CanvasProject[]): CanvasProject[] {
    const projects = new Map<string, CanvasProject>();
    [...localProjects, ...remoteProjects].map((project) => migrateCanvasProject(project)).forEach((project) => {
        const previous = projects.get(project.id);
        const projectTime = Date.parse(project.updatedAt || "") || 0;
        const previousTime = Date.parse(previous?.updatedAt || "") || 0;
        const projectRevision = project.operationState.revision;
        const previousRevision = previous?.operationState.revision ?? -1;
        if (
            !previous ||
            projectRevision > previousRevision ||
            (projectRevision === previousRevision && projectTime >= previousTime)
        ) {
            projects.set(project.id, project);
        }
    });
    return Array.from(projects.values()).sort(
        (a, b) =>
            Date.parse(b.updatedAt || "") -
            Date.parse(a.updatedAt || ""),
    );
}

function mergeDesktopCanvasProjects(
    desktopProjects: CanvasProject[],
    localProjects: CanvasProject[],
): CanvasProject[] {
    const localById = new Map(localProjects.map((project) => [project.id, project]));
    const projects = new Map(localProjects.map((project) => [project.id, project]));

    desktopProjects.forEach((desktopProject) => {
        const localProject = localById.get(desktopProject.id);
        if (deletedDesktopIds.has(desktopProject.id)) return;
        projects.set(desktopProject.id, {
            ...migrateCanvasProject(pendingProjects.get(desktopProject.id) || desktopProject),
            viewport: localProject?.viewport || desktopProject.viewport,
            sidePanel: localProject?.sidePanel || desktopProject.sidePanel,
            agentPanel: localProject?.agentPanel || desktopProject.agentPanel,
        });
    });

    return Array.from(projects.values()).sort(
        (a, b) => Date.parse(b.updatedAt || "") - Date.parse(a.updatedAt || ""),
    );
}
