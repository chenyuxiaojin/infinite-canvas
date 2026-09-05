import type { CanvasConnection, CanvasNodeData } from "../types";

// Arrays are the source of truth. Rebuild only when a published snapshot changes,
// including undo/redo and remote edits; never retain DOM elements across renders.
export function createCanvasDragIndex() {
    let previousNodes: CanvasNodeData[] | undefined;
    let previousConnections: CanvasConnection[] | undefined;
    let nodesById = new Map<string, CanvasNodeData>();
    let adjacent = new Map<string, Set<CanvasConnection>>();
    return (nodes: CanvasNodeData[], connections: CanvasConnection[], movedIds: Iterable<string>) => {
        if (nodes !== previousNodes) {
            previousNodes = nodes;
            nodesById = new Map(nodes.map((node) => [node.id, node]));
        }
        if (connections !== previousConnections) {
            previousConnections = connections;
            adjacent = new Map();
            for (const connection of connections) {
                for (const id of [connection.fromNodeId, connection.toNodeId]) {
                    let edges = adjacent.get(id);
                    if (!edges) adjacent.set(id, edges = new Set());
                    edges.add(connection);
                }
            }
        }
        const affected = new Set<CanvasConnection>();
        for (const id of movedIds) for (const edge of adjacent.get(id) || []) affected.add(edge);
        return { nodesById, affected };
    };
}
