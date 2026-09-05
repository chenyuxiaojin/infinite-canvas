import type { CanvasConnection, CanvasNodeData } from "../types";

export function validateCanvasGraph(project: { nodes: CanvasNodeData[]; connections: CanvasConnection[] }) {
    if (!Array.isArray(project.nodes) || !Array.isArray(project.connections)) throw new Error("画布缺少节点或连线列表");
    const nodes = new Set<string>();
    for (const node of project.nodes) {
        if (!node.id || nodes.has(node.id)) throw new Error("画布存在空白或重复节点编号");
        nodes.add(node.id);
    }
    const connections = new Set<string>();
    for (const connection of project.connections) {
        if (!connection.id || connections.has(connection.id)) throw new Error("画布存在空白或重复连线编号");
        if (!nodes.has(connection.fromNodeId) || !nodes.has(connection.toNodeId)) throw new Error(`连线 ${connection.id} 的节点不存在，原编辑已保留`);
        connections.add(connection.id);
    }
}
