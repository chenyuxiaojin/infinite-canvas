"use client";

import type { CSSProperties } from "react";
import { Modal, Tag, Timeline } from "antd";
import { useVersionCheck } from "@/hooks/use-version-check";
import { APP_VERSION } from "@/constant/env";

function getTagColor(type: string) {
    if (type === "新增") return "green";
    if (type === "修复") return "red";
    if (type === "调整") return "blue";
    if (type === "文档") return "purple";
    return "default";
}

function getReleaseTitle(version: string) {
    return version === "Unreleased" ? "未发布" : version.startsWith("v0.") ? `上游历史 ${version}` : version;
}

function renderReleaseContent(content: string) {
    return content.split(/(`[^`]+`)/g).map((part, index) => {
        if (part.startsWith("`") && part.endsWith("`")) {
            return (
                <code key={index} className="rounded border border-stone-200 bg-stone-100 px-1 py-0.5 font-mono text-[12px] text-stone-800 dark:border-stone-700 dark:bg-stone-900 dark:text-stone-200">
                    {part.slice(1, -1)}
                </code>
            );
        }
        return part;
    });
}

type VersionReleaseModalProps = {
    className?: string;
    style?: CSSProperties;
};

export function VersionReleaseModal({ className, style }: VersionReleaseModalProps) {
    const { open, setOpen, openReleaseModal, releases } = useVersionCheck();

    return (
        <>
            <button
                type="button"
                className={className || "shrink-0 cursor-pointer text-xs font-medium text-stone-500 transition hover:text-stone-950 dark:text-stone-400 dark:hover:text-white"}
                style={style}
                onClick={openReleaseModal}
                title="查看本机版本记录"
            >
                <span className="relative inline-flex">
                    {APP_VERSION}
                </span>
            </button>
            <Modal title="小陈的画布 · 版本记录" open={open} width={680} centered footer={null} onCancel={() => setOpen(false)}>
                <div className="mb-5 rounded-lg border border-stone-200 p-3 dark:border-stone-800">
                    <div className="text-xs text-stone-500 dark:text-stone-400">当前版本</div>
                    <div className="mt-1 text-base font-semibold text-stone-950 dark:text-stone-100">{APP_VERSION}</div>
                    <p className="mt-2 text-xs text-stone-500">独立本机版本，不跟随上游检查更新。</p>
                </div>
                <div className="max-h-[56vh] overflow-y-auto pr-2">
                    <Timeline
                        items={releases.map((release) => ({
                            content: (
                                <div>
                                    <div className="flex flex-wrap items-center gap-2">
                                        <span className="text-sm font-semibold text-stone-950 dark:text-stone-100">{getReleaseTitle(release.version)}</span>
                                        <span className="text-xs text-stone-500 dark:text-stone-400">{release.date}</span>
                                        <div className="flex min-w-0 items-center gap-1.5">
                                            {release.version === APP_VERSION ? <Tag>当前</Tag> : null}
                                        </div>
                                    </div>
                                    <div className="mt-2 space-y-1.5">
                                        {release.items.map((item, index) => (
                                            <div key={`${release.version}-${index}`} className="flex items-start gap-2 text-sm leading-6 text-stone-700 dark:text-stone-300">
                                                <Tag color={getTagColor(item.type)} className="m-0 mt-0.5 shrink-0 whitespace-nowrap">
                                                    {item.type}
                                                </Tag>
                                                <span className="min-w-0 flex-1">{renderReleaseContent(item.content)}</span>
                                            </div>
                                        ))}
                                    </div>
                                </div>
                            ),
                        }))}
                    />
                </div>
            </Modal>
        </>
    );
}
