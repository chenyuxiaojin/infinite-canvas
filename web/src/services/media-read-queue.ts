// Limit concurrent full-original reads; queued abandoned views do no I/O.
export function createMediaReadQueue(limit = 2) {
    const queue: Array<() => Promise<void>> = [];
    let active = 0;
    const drain = () => {
        while (active < limit && queue.length) {
            active++;
            void queue.shift()!().finally(() => { active--; drain(); });
        }
    };
    return (read: () => Promise<string>, alive: () => boolean) => new Promise<string>((resolve, reject) => {
        queue.push(async () => {
            if (!alive()) { resolve(""); return; }
            try { resolve(await read()); } catch (error) { reject(error); }
        });
        queueMicrotask(drain);
    });
}
