// Finite read-only macOS process sampling. Outputs are benchmark evidence, not UI FPS.
import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";

const [label, durationArg, directory, ...pidArgs] = process.argv.slice(2);
const duration = Number(durationArg);
if (!label || !(duration > 0 && duration <= 60) || !path.isAbsolute(directory)
    || !pidArgs.length || pidArgs.some(pid => !/^\d+$/.test(pid))) {
    throw new Error("Usage: node native-process-sampler.mjs LABEL SECONDS ABS_OUTPUT_DIR PID...");
}
const startedAt = new Date().toISOString();
const started = performance.now();
const samples = [];
while (performance.now() - started < duration * 1000) {
    const result = execFileSync("/bin/ps", ["-p", pidArgs.join(","), "-o", "pid=,ppid=,stat=,%cpu=,rss=,time="], { encoding: "utf8" });
    const processes = result.trim().split("\n").filter(Boolean).map(line => {
        const [pid, ppid, state, cpuPercent, rssKiB, cpuTime] = line.trim().split(/\s+/);
        return { pid: Number(pid), ppid: Number(ppid), state, cpuPercent: Number(cpuPercent), rssKiB: Number(rssKiB), cpuTime };
    });
    samples.push({ elapsedMs: performance.now() - started, processes });
    await new Promise(resolve => setTimeout(resolve, 200));
}
const cpuSeconds = value => value.split(":").reverse().reduce((sum, part, index) => sum + Number(part) * 60 ** index, 0);
const summary = pidArgs.map(Number).map(pid => {
    const rows = samples.flatMap(sample => sample.processes.filter(item => item.pid === pid));
    if (!rows.length) return { pid, missing: true };
    return {
        pid, firstRssMiB: rows[0].rssKiB / 1024, lastRssMiB: rows.at(-1).rssKiB / 1024,
        maxRssMiB: Math.max(...rows.map(row => row.rssKiB / 1024)),
        maxSampledCpuPercent: Math.max(...rows.map(row => row.cpuPercent)),
        totalCpuSeconds: cpuSeconds(rows.at(-1).cpuTime) - cpuSeconds(rows[0].cpuTime),
    };
});
mkdirSync(directory, { recursive: true });
const file = path.join(directory, `process-${label}-${startedAt.replaceAll(":", "-")}.json`);
writeFileSync(file, JSON.stringify({ label, startedAt, duration, intervalMs: 200, summary, samples }, null, 2), { flag: "wx" });
console.log(JSON.stringify({ file, label, summary }, null, 2));
