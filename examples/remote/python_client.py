#!/usr/bin/env python3
"""
Exemplo de cliente Python para controle remoto do Code Terminal.

Requisitos:
    pip install requests

Uso:
    python python_client.py --socket /tmp/code-terminal.sock list-tabs
    python python_client.py --host 127.0.0.1 --port 9876 new-tab --title "Build"
"""

import argparse
import json
import socket
import sys
from typing import Optional, Dict, Any


class CodeTerminalClient:
    """Cliente JSON-RPC para Code Terminal."""

    def __init__(self, socket_path: Optional[str] = None, 
                 host: str = "127.0.0.1", port: int = 9876):
        self.socket_path = socket_path
        self.host = host
        self.port = port
        self.sock: Optional[socket.socket] = None
        self.request_id = 0

    def connect(self) -> None:
        """Conecta ao servidor."""
        if self.socket_path:
            self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            self.sock.connect(self.socket_path)
        else:
            self.sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            self.sock.connect((self.host, self.port))

    def disconnect(self) -> None:
        """Desconecta do servidor."""
        if self.sock:
            self.sock.close()
            self.sock = None

    def call(self, method: str, params: Optional[Dict[str, Any]] = None) -> Any:
        """Chama um método JSON-RPC."""
        if not self.sock:
            raise RuntimeError("Not connected")

        self.request_id += 1
        request = {
            "jsonrpc": "2.0",
            "id": self.request_id,
            "method": method,
            "params": params or {}
        }

        # Envia requisição
        request_json = json.dumps(request) + "\n"
        self.sock.send(request_json.encode())

        # Recebe resposta
        response_data = b""
        while not response_data.endswith(b"\n"):
            chunk = self.sock.recv(4096)
            if not chunk:
                break
            response_data += chunk

        response = json.loads(response_data.decode())

        if "error" in response:
            raise RuntimeError(f"RPC Error: {response['error']['message']}")

        return response.get("result")

    def ping(self) -> str:
        """Testa conexão."""
        return self.call("ping")

    def list_tabs(self) -> list:
        """Lista abas."""
        return self.call("tab.list")

    def new_tab(self, title: Optional[str] = None) -> int:
        """Cria nova aba."""
        params = {}
        if title:
            params["title"] = title
        return self.call("tab.new", params)

    def close_tab(self, index: int) -> None:
        """Fecha aba."""
        self.call("tab.close", {"index": index})

    def goto_tab(self, index: int) -> None:
        """Muda para aba."""
        self.call("tab.goto", {"index": index})

    def send_input(self, text: str) -> None:
        """Envia texto."""
        self.call("terminal.input", {"text": text})

    def get_size(self) -> dict:
        """Retorna tamanho do terminal."""
        return self.call("terminal.size")

    def list_methods(self) -> list:
        """Lista métodos disponíveis."""
        return self.call("rpc.listMethods")


def main():
    parser = argparse.ArgumentParser(description="Code Terminal Python Client")
    parser.add_argument("--socket", help="Unix socket path")
    parser.add_argument("--host", default="127.0.0.1", help="TCP host")
    parser.add_argument("--port", type=int, default=9876, help="TCP port")
    
    subparsers = parser.add_subparsers(dest="command", help="Command")

    # ping
    subparsers.add_parser("ping", help="Test connection")

    # list-tabs
    subparsers.add_parser("list-tabs", help="List tabs")

    # new-tab
    new_tab_parser = subparsers.add_parser("new-tab", help="Create new tab")
    new_tab_parser.add_argument("--title", help="Tab title")

    # close-tab
    close_tab_parser = subparsers.add_parser("close-tab", help="Close tab")
    close_tab_parser.add_argument("index", type=int, help="Tab index")

    # goto-tab
    goto_parser = subparsers.add_parser("goto-tab", help="Switch to tab")
    goto_parser.add_argument("index", type=int, help="Tab index")

    # input
    input_parser = subparsers.add_parser("input", help="Send text input")
    input_parser.add_argument("text", help="Text to send")

    # size
    subparsers.add_parser("size", help="Get terminal size")

    # methods
    subparsers.add_parser("methods", help="List available methods")

    args = parser.parse_args()

    if not args.command:
        parser.print_help()
        sys.exit(1)

    # Cria cliente
    client = CodeTerminalClient(
        socket_path=args.socket,
        host=args.host,
        port=args.port
    )

    try:
        client.connect()

        if args.command == "ping":
            result = client.ping()
            print(f"Response: {result}")

        elif args.command == "list-tabs":
            tabs = client.list_tabs()
            if tabs:
                print(f"{'Index':<6} {'Active':<8} {'Title':<20} {'CWD'}")
                print("-" * 60)
                for tab in tabs:
                    active = "*" if tab.get("is_active") else ""
                    print(f"{tab['index']:<6} {active:<8} {tab['title']:<20} {tab.get('cwd', '-')}")
            else:
                print("No tabs")

        elif args.command == "new-tab":
            index = client.new_tab(args.title)
            print(f"Created tab {index}")

        elif args.command == "close-tab":
            client.close_tab(args.index)
            print(f"Closed tab {args.index}")

        elif args.command == "goto-tab":
            client.goto_tab(args.index)
            print(f"Switched to tab {args.index}")

        elif args.command == "input":
            client.send_input(args.text)
            print(f"Sent: {args.text}")

        elif args.command == "size":
            size = client.get_size()
            print(f"Terminal size: {size['cols']}x{size['rows']}")

        elif args.command == "methods":
            methods = client.list_methods()
            for method in methods:
                print(f"  - {method}")

    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)
    finally:
        client.disconnect()


if __name__ == "__main__":
    main()
