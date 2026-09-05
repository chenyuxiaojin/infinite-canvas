"""Opt-in CLI startup probe: NEVER sends a prompt/model request; uses a temporary workspace."""
import argparse
import http.server
import json
import pathlib
import queue
import subprocess
import tempfile
import threading
import uuid

parser = argparse.ArgumentParser()
parser.add_argument('--provider', choices=['grok', 'antigravity'], required=True)
args = parser.parse_args()
root = pathlib.Path(__file__).resolve().parents[2]
calls = []

class MCP(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass
    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers.get('Content-Length', 0))))
        method = request.get('method')
        calls.append(method)
        if 'id' not in request:
            self.send_response(202)
            self.end_headers()
            return
        if method == 'initialize':
            result = {'protocolVersion': '2025-06-18', 'capabilities': {'tools': {}}, 'serverInfo': {'name': 'canvas-probe', 'version': '1'}}
        elif method == 'tools/list':
            result = {'tools': [{'name': 'get_canvas_summary', 'description': 'Read only probe', 'inputSchema': {'type': 'object', 'properties': {}}}]}
        else:
            result = {}
        data = json.dumps({'jsonrpc': '2.0', 'id': request['id'], 'result': result}).encode()
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(data)))
        self.end_headers()
        self.wfile.write(data)

server = http.server.HTTPServer(('127.0.0.1', 0), MCP)
threading.Thread(target=server.serve_forever, daemon=True).start()
with tempfile.TemporaryDirectory(prefix='canvas-cli-probe-') as directory:
    cwd = pathlib.Path(directory)
    endpoint = f'http://127.0.0.1:{server.server_port}/mcp'
    executable = pathlib.Path.home() / '.local/bin' / ('grok' if args.provider == 'grok' else 'agy')
    if args.provider == 'grok':
        profile = cwd / 'profile.md'
        profile.write_text((root / 'desktop/src-tauri/src/canvas_grok_agent.md').read_text())
        command = [str(executable), '--no-auto-update', 'agent', '--no-leader', '--agent-profile', str(profile), 'stdio']
    else:
        agents = pathlib.Path.home() / '.gemini/config/agents'
        agents.mkdir(parents=True, exist_ok=True)
        name = 'canvas-probe-' + uuid.uuid4().hex
        profile = agents / (name + '.md')
        profile.write_text(f'---\nname: {name}\ndescription: Canvas startup probe\nmainAgent: true\nsubagent: false\ninheritCustomizations: false\ncommandExecutionPolicy: off\ntools: [call_mcp_tool, finish]\nmcpServers:\n  - name: xiaochens_canvas_sidepanel\n    serverUrl: {endpoint}\n---\nCanvas only.\n')
        profile.chmod(0o600)
        command = [str(executable), '--agent', name, '--disable-slash-commands', '--input-format', 'stream-json', '--output-format', 'stream-json']
    proc = subprocess.Popen(command, cwd=cwd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    output = queue.Queue()
    errors = []
    def read_output():
        for line in proc.stdout:
            output.put(json.loads(line))
        output.put(None)
    def read_errors():
        for line in proc.stderr:
            if 'ERROR' in line or line.startswith('error:'):
                errors.append(line.strip())
    threading.Thread(target=read_output, daemon=True).start()
    threading.Thread(target=read_errors, daemon=True).start()
    def send(message):
        proc.stdin.write(json.dumps(message) + '\n')
        proc.stdin.flush()
    def request(number, method, params):
        send({'jsonrpc': '2.0', 'id': number, 'method': method, 'params': params})
        while True:
            message = output.get(timeout=30)
            if message is None:
                raise RuntimeError('CLI exited: ' + ' | '.join(errors[-2:]))
            if message.get('id') == number:
                if 'error' in message:
                    raise RuntimeError(str(message['error']))
                return message['result']
    try:
        if args.provider == 'grok':
            init = request(1, 'initialize', {'protocolVersion': 1, 'clientCapabilities': {}})
            request(2, 'authenticate', {'methodId': 'cached_token', '_meta': {'headless': True}})
            session = request(3, 'session/new', {'cwd': directory, 'mcpServers': [{'type': 'http', 'name': 'xiaochens_canvas_sidepanel', 'url': endpoint, 'headers': []}]})
            print(json.dumps({'provider': 'grok', 'initialize': True, 'cached_auth': True, 'session_created': bool(session.get('sessionId')), 'capabilities': init.get('agentCapabilities'), 'session_result_keys': list(session), 'models': session.get('models'), 'model_config': [v for v in session.get('configOptions',[]) if v.get('category') == 'model'], 'mcp_methods': calls, 'model_prompts_sent': 0}, ensure_ascii=False))
        else:
            # Explicitly unsupported control event: validation returns ERROR with zero tokens.
            # It cannot request a model turn, unlike a user event (even an empty user prompt).
            send({'event': 'control_request'})
            while True:
                message = output.get(timeout=30)
                if message is None:
                    raise RuntimeError('CLI exited before result')
                if message.get('event') == 'init':
                    print(json.dumps({'provider': 'antigravity', 'init': True, 'tools': message['init']['tools'], 'mcp_methods': calls}))
                if message.get('event') == 'result':
                    result = message['result']
                    assert result['num_turns'] == 0 and result['usage']['total_tokens'] == 0
                    print(json.dumps({'expected_validation_error': result['error'], 'usage': result['usage'], 'model_prompts_sent': 0}))
                    break
    finally:
        if proc.poll() is None:
            proc.kill()
        proc.wait(timeout=5)
        server.shutdown()
        if args.provider == "antigravity": profile.unlink(missing_ok=True)
