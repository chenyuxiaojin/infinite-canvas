// Read-only comparison of two explicit SQLite snapshots; never reads credentials
// into the report. Usage: node verify-repair-data.mjs before.db after.db report.json
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, writeFileSync } from "node:fs";
import { isAbsolute } from "node:path";
import assert from "node:assert/strict";

const [before, after, reportPath] = process.argv.slice(2);
for (const path of [before, after, reportPath]) assert.ok(path && isAbsolute(path), "Use explicit absolute paths");
assert.ok(existsSync(before) && existsSync(after), "Both snapshots must exist");
assert.ok(!existsSync(reportPath), "The evidence report must be a new file");
const quote = value => `"${value.replaceAll('"', '""')}"`;
function query(database, sql) {
    const output = execFileSync("/usr/bin/sqlite3", ["-json", `file:${database}?mode=ro`, sql], {
        encoding: "utf8", maxBuffer: 256 * 1024 * 1024,
    });
    return output.trim() ? JSON.parse(output) : [];
}
function rowDigest(rows) {
    const hash = createHash("sha256");
    for (const row of rows.map(row => JSON.stringify(row)).sort()) hash.update(row).update("\n");
    return hash.digest("hex");
}
const schemaSql = "SELECT type,name,tbl_name,sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY type,name";
const beforeSchema = query(before, schemaSql);
const afterSchema = query(after, schemaSql);
const report = {
    before, after, checkedAt: new Date().toISOString(),
    integrity: { before: query(before, "PRAGMA integrity_check"), after: query(after, "PRAGMA integrity_check") },
    schemaUnchanged: JSON.stringify(beforeSchema) === JSON.stringify(afterSchema),
    tables: [], canvasChanges: [],
    limitation: "SQLite snapshots only; does not compare WebKit/IndexedDB blobs or raw image files",
};
for (const { name } of beforeSchema.filter(entry => entry.type === "table")) {
    const a = query(before, `SELECT * FROM ${quote(name)}`);
    const b = query(after, `SELECT * FROM ${quote(name)}`);
    const aHash = rowDigest(a), bHash = rowDigest(b);
    report.tables.push({ name, beforeRows: a.length, afterRows: b.length, beforeHash: aHash, afterHash: bHash, unchanged: aHash === bHash });
    if (name !== "canvas_projects" || aHash === bHash) continue;
    const key = row => JSON.stringify([row.user_id, row.id]);
    const all = new Set([...a.map(key), ...b.map(key)]);
    for (const id of all) {
        const oldRow = a.find(row => key(row) === id), newRow = b.find(row => key(row) === id);
        if (JSON.stringify(oldRow) === JSON.stringify(newRow)) continue;
        if (!oldRow || !newRow) {
            report.canvasChanges.push({ id: (oldRow || newRow).id, kind: oldRow ? "removed" : "added" });
            continue;
        }
        const oldProject = JSON.parse(oldRow.project_data), newProject = JSON.parse(newRow.project_data);
        const changedKeys = [...new Set([...Object.keys(oldProject), ...Object.keys(newProject)])]
            .filter(key => JSON.stringify(oldProject[key]) !== JSON.stringify(newProject[key]));
        report.canvasChanges.push({
            id: oldRow.id, kind: "changed",
            changedColumns: Object.keys(oldRow).filter(key => oldRow[key] !== newRow[key]),
            changedProjectKeys: changedKeys,
            nodesUnchanged: JSON.stringify(oldProject.nodes) === JSON.stringify(newProject.nodes),
            connectionsUnchanged: JSON.stringify(oldProject.connections) === JSON.stringify(newProject.connections),
        });
    }
}
writeFileSync(reportPath, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx", mode: 0o600 });
console.log(JSON.stringify({ reportPath, schemaUnchanged: report.schemaUnchanged, integrity: report.integrity,
    tables: report.tables.map(({ name, beforeRows, afterRows, unchanged }) => ({ name, beforeRows, afterRows, unchanged })),
    canvasChanges: report.canvasChanges,
}));
