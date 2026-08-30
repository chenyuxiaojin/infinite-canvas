import { create } from "zustand";
import { persist, type PersistStorage, type StorageValue } from "zustand/middleware";

import { nanoid } from "nanoid";
import { localForageStorage } from "@/lib/localforage-storage";
import { listCanvasProjects, saveCanvasProject, syncCanvasProjects } from "@/services/api/canvas-tasks";
import { fetchUserConfig } from "@/services/api/user-config";
import { useUserStore } from "@/stores/use-user-store";
import { isDesktopRuntime, listDesktopCanvasProjects, saveDesktopCanvasProject } from "@/services/desktop-runtime";
import type { CanvasBackgroundMode } from "@/lib/canvas-theme";
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

export const DEFAULT_CANVAS_SIDE_PANEL: CanvasSidePanelState = { open: true, width: 280 };
export const DEFAULT_CANVAS_AGENT_PANEL: CanvasSidePanelState = { open: false, width: 390 };

export type CanvasProject = {
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
    createProject: (title?: string, options?: { agentConfig?: CanvasAgentConfig; pendingAgentRequest?: CanvasPendingAgentRequest }) => string;
    importProject: (project: Partial<CanvasProject>) => string;
    openProject: (id: string) => CanvasProject | null;
    renameProject: (id: string, title: string) => void;
    deleteProjects: (ids: string[]) => void;
    updateProject: (id: string, patch: Partial<Pick<CanvasProject, "nodes" | "connections" | "chatSessions" | "activeChatId" | "agentConfig" | "autoTitlePending" | "backgroundMode" | "showImageInfo" | "viewport" | "sidePanel" | "agentPanel" | "pendingAgentRequest">>) => void;
    applyOperationBatch: (batch: CanvasOperationBatch) => CanvasOperationOutcome<CanvasProject> | null;
    refreshFromDesktop: () => Promise<void>;
    syncWithRemote: (token: string, syncEnabled: boolean) => Promise<void>;
    setSyncEnabled: (enabled: boolean) => void;
};

const initialViewport: ViewportTransform = { x: 0, y: 0, k: 1 };
const CANVAS_STORE_KEY = "infinite-canvas:canvas_store";
type PersistedCanvasState = Pick<CanvasStore, "projects">;
let saveTimer: ReturnType<typeof setTimeout> | null = null;
let queuedPersistState: PersistedCanvasState | null = null;
let accountCanvasSyncEnabled = false;
const projectSaveTimers = new Map<string, ReturnType<typeof setTimeout>>();

function waitForUserStoreHydration() {
    if (useUserStore.persist.hasHydrated()) return Promise.resolve();

    return new Promise<void>((resolve) => {
        let unsubscribe = () => {};
        unsubscribe = useUserStore.persist.onFinishHydration(() => {
            unsubscribe();
            resolve();
        });
        if (useUserStore.persist.hasHydrated()) {
            unsubscribe();
            resolve();
        }
    });
}

function queueProjectSave(project: CanvasProject) {
    const desktop = isDesktopRuntime();
    const token = useUserStore.getState().token;
    const syncEnabled = accountCanvasSyncEnabled;
    const previous = projectSaveTimers.get(project.id);
    if (previous) clearTimeout(previous);

    projectSaveTimers.set(
        project.id,
        setTimeout(() => {
            projectSaveTimers.delete(project.id);
            if (desktop) {
                void saveDesktopCanvasProject<CanvasProject>(project)
                    .then((saved) => {
                        adoptAuthoritativeDesktopProject(saved);
                        useCanvasStore.setState({
                            desktopPersistenceStatus: "database",
                            desktopPersistenceError: null,
                        });
                    })
                    .catch((error) => {
                        useCanvasStore.setState({
                            desktopPersistenceStatus: "error",
                            desktopPersistenceError: error instanceof Error ? error.message : String(error),
                        });
                    });
                return;
            }
            if (
                !token ||
                !syncEnabled ||
                !accountCanvasSyncEnabled ||
                useUserStore.getState().token !== token
            ) {
                return;
            }
            void saveCanvasProject(token, project).catch(() => undefined);
        }, 400),
    );
}

function adoptAuthoritativeDesktopProject(source: CanvasProject) {
    const saved = migrateCanvasProject(source);
    const current = useCanvasStore.getState().projects.find((project) => project.id === saved.id);
    if (!current) {
        useCanvasStore.setState((state) => ({ projects: mergeCanvasProjects([saved], state.projects) }));
        return;
    }
    const merged = mergeDesktopProject(saved, current);
    if (merged !== current) {
        useCanvasStore.setState((state) => ({
            projects: state.projects.map((project) => project.id === saved.id ? merged : project),
        }));
    }
}

function mergeDesktopProject(saved: CanvasProject, current: CanvasProject) {
    const savedRevision = saved.operationState.revision;
    const currentRevision = current.operationState.revision;
    if (savedRevision > currentRevision) return saved;
    if (savedRevision < currentRevision) return current;
    const operationStateChanged = JSON.stringify(saved.operationState) !== JSON.stringify(current.operationState);
    if (operationStateChanged) {
        return {
            ...current,
            title: saved.title,
            nodes: saved.nodes,
            connections: saved.connections,
            updatedAt: saved.updatedAt,
            operationState: saved.operationState,
        };
    }
    return (Date.parse(saved.updatedAt || "") || 0) > (Date.parse(current.updatedAt || "") || 0)
        ? saved
        : current;
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

const canvasStorage: PersistStorage<CanvasStore> = {
    getItem: async (name) => {
        await waitForUserStoreHydration();
        const localValue = await localForageStorage.getItem(name);
        const token = useUserStore.getState().token;
        const localParsed = localValue
            ? (JSON.parse(localValue) as StorageValue<CanvasStore>)
            : null;
        const localProjects = (
            (localParsed?.state as PersistedCanvasState)?.projects || []
        ).map((project) => migrateCanvasProject(project));
        const localHasData =
            Array.isArray(localProjects) && localProjects.length > 0;

        if (isDesktopRuntime()) {
            try {
                const desktopProjects = await listDesktopCanvasProjects<CanvasProject>();
                useCanvasStore.setState({
                    desktopPersistenceStatus: "database",
                    desktopPersistenceError: null,
                });
                const desktopById = new Map(
                    desktopProjects.map((project) => [project.id, project]),
                );
                const projects = mergeCanvasProjects(
                    desktopProjects,
                    localProjects,
                );
                await Promise.all(
                    localProjects
                        .filter((project) => {
                            const desktop = desktopById.get(project.id);
                            return !desktop
                                || project.operationState.revision > desktop.operationState.revision
                                || (
                                    project.operationState.revision === desktop.operationState.revision
                                    && Date.parse(project.updatedAt || "") > Date.parse(desktop.updatedAt || "")
                                );
                        })
                        .map((project) => saveDesktopCanvasProject(project)),
                );
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
                useCanvasStore.setState({
                    desktopPersistenceStatus: "error",
                    desktopPersistenceError: error instanceof Error ? error.message : String(error),
                });
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
                    await localForageStorage.setItem(name, JSON.stringify(parsed));
                    return parsed;
                }

                if (
                    remoteProjects.length > 0 &&
                    (accountCanvasSyncEnabled || !localHasData)
                ) {
                    const nextState = { projects: remoteProjects.map((project) => migrateCanvasProject(project)) };
                    const parsed = {
                        state: nextState,
                        version: 0,
                    } as StorageValue<CanvasStore>;
                    queuedPersistState = nextState;
                    await localForageStorage.setItem(name, JSON.stringify(parsed));
                    return parsed;
                }
            } catch (error) {
                console.error("Failed to hydrate canvas projects from remote", error);
            }
        }

        if (!localParsed) return null;
        const nextState = { ...(localParsed.state as PersistedCanvasState), projects: localProjects };
        const migrated = { ...localParsed, state: nextState } as StorageValue<CanvasStore>;
        queuedPersistState = { projects: localProjects };
        await localForageStorage.setItem(name, JSON.stringify(migrated));
        return migrated;
    },

    setItem: (name, value) => {
        const nextState = value.state as PersistedCanvasState;
        if (queuedPersistState && queuedPersistState.projects === nextState.projects) {
            return;
        }
        queuedPersistState = nextState;
        if (saveTimer) clearTimeout(saveTimer);
        saveTimer = setTimeout(() => {
            saveTimer = null;
            void localForageStorage.setItem(name, JSON.stringify(value));
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
                const now = new Date().toISOString();
                const id = nanoid();
                const project = rebindCanvasProjectIdentity(migrateCanvasProject<CanvasProject>({
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
                        console.error("Failed to apply canvas UI operation batch", outcome.result.error);
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
                    updatedAt: operations.length ? nextProject.updatedAt : new Date().toISOString(),
                };
                set((state) => ({
                    projects: state.projects.map((item) => (item.id === id ? nextProject : item)),
                }));
                queueProjectSave(nextProject);
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
            refreshFromDesktop: async () => {
                if (!isDesktopRuntime()) return;
                try {
                    const desktopProjects = await listDesktopCanvasProjects<CanvasProject>();
                    set((state) => {
                        const currentById = new Map(state.projects.map((project) => [project.id, project]));
                        const desktopById = new Map(desktopProjects.map((project) => {
                            const saved = migrateCanvasProject(project);
                            const current = currentById.get(saved.id);
                            return [saved.id, current ? mergeDesktopProject(saved, current) : saved] as const;
                        }));
                        const retained = state.projects.map((project) => desktopById.get(project.id) || project);
                        const added = Array.from(desktopById.values()).filter((project) => !currentById.has(project.id));
                        return {
                            projects: [...added, ...retained],
                            desktopPersistenceStatus: "database" as const,
                            desktopPersistenceError: null,
                        };
                    });
                } catch (error) {
                    set({
                        desktopPersistenceStatus: "error",
                        desktopPersistenceError: error instanceof Error ? error.message : String(error),
                    });
                    throw error;
                }
            },
            syncWithRemote: async (token, syncEnabled) => {
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
                await localForageStorage.setItem(CANVAS_STORE_KEY, JSON.stringify({ state: nextState, version: 0 }));
            },
            setSyncEnabled: (enabled) => {
                accountCanvasSyncEnabled = enabled;
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
