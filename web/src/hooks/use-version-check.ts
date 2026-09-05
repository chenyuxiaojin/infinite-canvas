import { useMemo, useState } from "react";
import type { ReleaseInfo } from "@/lib/release";

function readLocalReleases(): ReleaseInfo[] {
    try {
        return JSON.parse(process.env.NEXT_PUBLIC_APP_RELEASES || "[]");
    } catch {
        return [];
    }
}

// This personal fork has no remote update feed; upstream releases are not its updates.
export function useVersionCheck() {
    const [open, setOpen] = useState(false);
    const releases = useMemo(readLocalReleases, []);
    return { open, setOpen, openReleaseModal: () => setOpen(true), releases };
}
