// Finite, local-only native PTY probe. No model/network/configuration access.
// node /absolute/path/native-terminal-probe.mjs burst 1 --report-dir /existing/backup/dir
// node /absolute/path/native-terminal-probe.mjs stream 15 512 --report-dir /existing/backup/dir
import { writeFileSync, statSync } from "node:fs";
import path from "node:path";
import { StringDecoder } from "node:string_decoder";

const args = process.argv.slice(2);
const flag = args.indexOf("--report-dir");
const reportDir = flag >= 0 ? args[flag + 1] : undefined;
if (flag >= 0) args.splice(flag, 2);
const largeBurstIndex = args.indexOf("--large-burst");
const largeBurst = largeBurstIndex >= 0;
if (largeBurst) args.splice(largeBurstIndex, 1);
const [mode, amountText, rateText] = args;
const amount = Number(amountText);
const rate = Number(rateText ?? 512);
if (!reportDir || !path.isAbsolute(reportDir) || !statSync(reportDir).isDirectory()) {
    throw new Error("--report-dir must be an explicit existing absolute directory");
}
if (!(mode === "burst" && amount > 0 && amount <= (largeBurst ? 64 : 8))
    && !(mode === "stream" && amount > 0 && amount <= 20 && rate > 0 && rate <= 512)) {
    throw new Error("Use burst 0<MiB<=8 (explicit --large-burst allows up to 64), or stream 0<seconds<=20 0<KiBps<=512");
}
if (!process.stdin.isTTY || !process.stdout.isTTY) throw new Error("Run this probe inside the App terminal PTY");

const startedAt = new Date().toISOString();
const start = performance.now();
const reportPath = path.join(reportDir, `terminal-probe-${startedAt.replace(/[:.]/g, "-")}-${process.pid}.json`);
const report = {
    schema: "native-terminal-probe-v1", mode, amount, rateKiBps: mode === "stream" ? rate : null,
    startedAt, pid: process.pid, ppid: process.ppid, cwd: process.cwd(),
    node: process.version, reportPath,
    metricMeaning: "DSR is PTY->Tauri->xterm parser->PTY round trip, NOT paint, FPS, or external key-to-arrival latency",
    payloadBytes: 0, wireBytes: 0, producerMs: 0, producerDrainWaits: 0, producerDrainMs: 0,
    completedRows: 0, expectedRowPrefix: "QA", dsr: [], keys: [], unicode: {}, aborted: false, error: null,
};
const originalRaw = process.stdin.isRaw;
const decoder = new StringDecoder("utf8");
let input = "";
let pendingDsr;
let dsrChain = Promise.resolve();
let dsrTimedOut = false;
let finishing = false;
const now = () => performance.now() - start;
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

function abort(reason) {
    report.aborted = true;
    report.abortReason = reason;
    if (pendingDsr) pendingDsr.finish({ status: "aborted", reason });
}

async function write(data, payload = false) {
    if (report.aborted) return false;
    const bytes = Buffer.isBuffer(data) ? data : Buffer.from(data);
    report.wireBytes += bytes.length;
    if (payload) report.payloadBytes += bytes.length;
    if (process.stdout.write(bytes)) return true;
    const drainStart = performance.now();
    report.producerDrainWaits++;
    await new Promise((resolve, reject) => {
        const timeout = setTimeout(() => done(new Error("stdout drain timed out after 5000 ms")), 5000);
        const done = error => {
            clearTimeout(timeout);
            process.stdout.off("drain", drained);
            process.stdout.off("error", failed);
            error ? reject(error) : resolve();
        };
        const drained = () => done();
        const failed = error => done(error);
        process.stdout.once("drain", drained);
        process.stdout.once("error", failed);
    });
    report.producerDrainMs += performance.now() - drainStart;
    return !report.aborted;
}

function query(label, keyIndex) {
    const run = async () => {
        if (finishing || report.aborted || dsrTimedOut) return;
        const sentMs = now();
        const result = await new Promise(resolve => {
            const timer = setTimeout(() => {
                dsrTimedOut = true; // DSR has no request IDs: never misattribute a late reply.
                pendingDsr?.finish({ status: "timeout", timeoutMs: 5000 });
            }, 5000);
            pendingDsr = {
                finish(values) {
                    clearTimeout(timer);
                    pendingDsr = undefined;
                    resolve({ label, keyIndex, sentMs, finishedMs: now(), ...values });
                },
            };
            void write("\x1b[6n").catch(error => pendingDsr?.finish({ status: "write_error", error: String(error) }));
        });
        if (result.status === "ok") result.roundTripMs = result.finishedMs - result.sentMs;
        report.dsr.push(result);
    };
    dsrChain = dsrChain.then(run);
    return dsrChain;
}

function key(text) {
    if (text.includes("\x03")) { abort("Ctrl+C received as raw input"); return; }
    if (finishing || report.aborted) return;
    const index = report.keys.length;
    const item = { index, receivedMs: now(), text, codePoints: Array.from(text, char => char.codePointAt(0)) };
    report.keys.push(item);
    void write(`\r\nQA_KEY ${index} arrival_ms=${item.receivedMs.toFixed(2)} text=${JSON.stringify(text)}\r\n`)
        .then(() => query(`key-${index}-after-echo`, index))
        .catch(error => { report.error = String(error); abort("key echo failed"); });
}

function onData(data) {
    input += decoder.write(data);
    while (input) {
        const response = /^\x1b\[(\d+);(\d+)R/.exec(input);
        if (response) {
            input = input.slice(response[0].length);
            pendingDsr?.finish({ status: "ok", cursorRow: Number(response[1]), cursorColumn: Number(response[2]) });
        } else if (/^\x1b(?:\[[\d;]*)?$/.test(input) && input.length < 64) {
            return;
        } else {
            const char = String.fromCodePoint(input.codePointAt(0));
            input = input.slice(char.length);
            key(char);
        }
    }
}

function makeRows(first, count) {
    return Buffer.from(Array.from({ length: count }, (_, offset) =>
        `QA ${String(first + offset).padStart(8, "0")} | 中文😀 | abcdefghijklmnopqrstuvwxyz 0123456789\r\n`).join(""));
}

process.stdin.setRawMode(true);
process.stdin.resume();
process.stdin.on("data", onData);
process.on("SIGINT", () => abort("SIGINT"));
process.on("SIGTERM", () => abort("SIGTERM"));
try {
    await write(`\r\nQA_PROBE_START mode=${mode} amount=${amount} pid=${process.pid}\r\n`);
    await query("baseline-before-output");
    const unicodeText = "QA_UNICODE 中文😀🧪重复重复 END\r\n";
    const unicodeBytes = Buffer.from(unicodeText);
    report.unicode = { expected: unicodeText.trimEnd(), producerByteWrites: unicodeBytes.length,
        caveat: "One-byte producer writes may still be coalesced by PTY/IPC; visual verification required" };
    for (const byte of unicodeBytes) {
        if (!await write(Buffer.from([byte]))) break;
        await sleep(2);
    }
    await query("after-unicode-byte-writes");
    const producerStart = performance.now();
    const bytesPerRow = makeRows(0, 1).length;
    if (mode === "burst") {
        const targetRows = Math.floor(amount * 1024 * 1024 / bytesPerRow);
        while (report.completedRows < targetRows && !report.aborted) {
            const count = Math.min(128, targetRows - report.completedRows);
            if (!await write(makeRows(report.completedRows, count), true)) break;
            report.completedRows += count;
        }
        report.producerMs = performance.now() - producerStart;
        await query("after-burst");
    } else {
        const totalTicks = Math.ceil(amount * 10);
        const targetTotalBytes = amount * rate * 1024;
        for (let tick = 0; tick < totalTicks && !report.aborted; tick++) {
            const rowsWanted = Math.floor(Math.min(targetTotalBytes, (tick + 1) * rate * 1024 / 10) / bytesPerRow);
            while (report.completedRows < rowsWanted && !report.aborted) {
                const count = Math.min(128, rowsWanted - report.completedRows);
                if (!await write(makeRows(report.completedRows, count), true)) break;
                report.completedRows += count;
            }
            if ((tick + 1) % 10 === 0) await query(`stream-second-${(tick + 1) / 10}`);
            const waitMs = producerStart + (tick + 1) * 100 - performance.now();
            if (waitMs > 0) await sleep(waitMs);
        }
        report.producerMs = performance.now() - producerStart;
        await query("after-stream");
    }
    await write(`\r\nQA_PAYLOAD_END rows=${report.completedRows} bytes=${report.payloadBytes}\r\n`);
    // A brief finite window permits manual keys even when burst parsing is fast.
    if (mode === "burst" && !report.aborted) {
        await write("QA_KEY_WINDOW 3 seconds; type ordinary keys or Ctrl+C\r\n");
        await sleep(3000);
    }
    await dsrChain;
} catch (error) {
    report.error = String(error);
} finally {
    finishing = true;
    if (pendingDsr) pendingDsr.finish({ status: "stopped" });
    process.stdin.off("data", onData);
    process.stdin.setRawMode(Boolean(originalRaw));
    process.stdin.pause();
    report.finishedAt = new Date().toISOString();
    report.totalMs = now();
    report.memory = process.memoryUsage();
    report.resourceUsage = process.resourceUsage();
    const ok = report.dsr.filter(item => item.status === "ok").map(item => item.roundTripMs).sort((a, b) => a - b);
    report.dsrSummary = { successful: ok.length, timeout: report.dsr.filter(item => item.status === "timeout").length,
        minMs: ok[0] ?? null, maxMs: ok.at(-1) ?? null, medianMs: ok[Math.floor(ok.length / 2)] ?? null };
    writeFileSync(reportPath, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx", mode: 0o600 });
    process.stdout.write(`\r\nQA_DONE aborted=${report.aborted} payload_bytes=${report.payloadBytes} producer_ms=${report.producerMs.toFixed(2)} dsr_ok=${ok.length} dsr_max_ms=${ok.at(-1)?.toFixed(2) ?? "NA"} keys=${report.keys.length}\r\nQA_REPORT ${reportPath}\r\n`);
}
