#!/usr/bin/env python3
"""Simple admin TCP client for AiDb node admin port.

Usage:
  ./deploy/admin_check.py --host 127.0.0.1 --port 8001 --cmd METRICS --cmd IS_LEADER --timeout 2

Prints responses (one line per command) prefixed by command.
"""
import argparse
import socket
import sys

parser = argparse.ArgumentParser()
parser.add_argument("--host", default="127.0.0.1")
parser.add_argument("--port", type=int, required=True)
parser.add_argument("--cmd", action="append", required=True)
parser.add_argument("--timeout", type=float, default=2.0)
args = parser.parse_args()

try:
    sock = socket.create_connection((args.host, args.port), timeout=args.timeout)
    sock.settimeout(args.timeout)
    # Use a simple send/recv loop for robustness
    for cmd in args.cmd:
        line = (cmd + "\n").encode("utf-8")
        try:
            sock.sendall(line)
        except Exception as e:
            print(f"ERR writing {cmd}: {e}")
            continue

        # Read until newline
        buf = bytearray()
        try:
            # Read until the socket times out; this allows multi-line responses
            while True:
                chunk = sock.recv(4096)
                if not chunk:
                    break
                buf.extend(chunk)
        except socket.timeout:
            if not buf:
                print(f"{cmd}: <timeout>")
                continue
        except Exception as e:
            print(f"{cmd}: <error {e}>")
            continue

        if not buf:
            print(f"{cmd}: <no response>")
        else:
            s = bytes(buf).decode("utf-8", errors="replace").strip()
            # Print each line of the response prefixed with the command for clarity
            for l in s.splitlines():
                print(f"{cmd}: {l}")
    sock.close()
except Exception as e:
    print(f"CONN_ERR: {e}")
    sys.exit(1)
