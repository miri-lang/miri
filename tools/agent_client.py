#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""
Reference client for the Miri agent protocol.

A dependency-free JSON-RPC 2.0 client for the `miri agent` command.
Implements the protocol framing (Content-Length headers + UTF-8 body)
and provides a high-level interface to agent methods.
"""

import json
import subprocess
import sys
from pathlib import Path


class AgentSession:
    """
    A session with a running `miri agent` subprocess.

    Spawns `miri agent` and implements the JSON-RPC 2.0 framing over
    stdin/stdout: writes Content-Length headers, reads exact byte counts,
    and handles the UTF-8 encoding/decoding transparently.

    Use as a context manager to automatically close the session.
    """

    def __init__(self, miri_binary):
        """Start a new session with the given Miri binary path."""
        self.miri_binary = Path(miri_binary)
        self.process = subprocess.Popen(
            [str(self.miri_binary), "agent"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=None,  # Let stderr pass through to the terminal (protocol specifies: stdout has frames, stderr for humans)
            text=False,  # We'll handle UTF-8 ourselves for precise byte counting
        )
        self.next_id = 1

    def __enter__(self):
        """Support context manager entry."""
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        """Support context manager exit; closes stdin to end the session."""
        self.close()
        return False

    def close(self):
        """Close stdin to signal end of session, wait for process to exit."""
        if self.process.stdin:
            self.process.stdin.close()
        self.process.wait()

    def _send_message(self, message_dict):
        """
        Send one framed JSON message.

        Encodes the message as UTF-8, writes a Content-Length header
        (in bytes), then a blank line, then the exact number of bytes.
        """
        body = json.dumps(message_dict, separators=(",", ":")).encode("utf-8")
        header = f"Content-Length: {len(body)}\r\n\r\n".encode("utf-8")
        self.process.stdin.write(header)
        self.process.stdin.write(body)
        self.process.stdin.flush()

    def _receive_message(self):
        """
        Read one framed JSON message.

        Reads headers line by line (looking for Content-Length),
        then reads exactly that many bytes of the body, and parses it as JSON.
        """
        # Read headers until blank line
        length = None
        while True:
            line_bytes = b""
            while True:
                ch = self.process.stdout.read(1)
                if not ch:
                    raise RuntimeError("session ended before providing Content-Length")
                line_bytes += ch
                if line_bytes.endswith(b"\n"):
                    break

            line = line_bytes.decode("utf-8").rstrip("\r\n")
            if not line:
                # Blank line ends headers
                break

            if line.startswith("Content-Length:"):
                try:
                    length_str = line.split(":", 1)[1].strip()
                    length = int(length_str)
                    if length < 0 or length > 64 * 1024 * 1024:
                        raise RuntimeError(
                            f"Content-Length {length} is outside valid range [0, 64MiB]"
                        )
                except ValueError:
                    raise RuntimeError(
                        f"Content-Length header has non-numeric value: {length_str}"
                    )

        if length is None:
            raise RuntimeError("no Content-Length header in response")

        # Read exactly that many bytes
        body = self.process.stdout.read(length)
        if len(body) != length:
            raise RuntimeError(
                f"expected {length} bytes but got {len(body)}"
            )

        return json.loads(body.decode("utf-8"))

    def call(self, method, params=None):
        """
        Call a method and return the response.

        Params should be a dict of parameter name to value.
        Returns the full JSON-RPC response (which includes both `result`
        and `error`; check one or the other).
        """
        if params is None:
            params = {}

        message = {
            "jsonrpc": "2.0",
            "id": self.next_id,
            "method": method,
            "params": params,
        }
        self.next_id += 1

        self._send_message(message)
        return self._receive_message()

    def initialize(self):
        """Call initialize and return the response."""
        return self.call("initialize", {})

    def check(self, path=None, source=None, verify_mir=False):
        """Call check and return the response.

        Either path or source (or both) must be provided.
        """
        params = {}
        if path is not None:
            params["path"] = path
        if source is not None:
            params["source"] = source
        if verify_mir:
            params["verifyMir"] = True
        return self.call("check", params)

    def view(self, path, fn=None, around=None):
        """Call view and return the response."""
        params = {"path": path}
        if fn is not None:
            params["fn"] = fn
        if around is not None:
            params["around"] = around
        return self.call("view", params)

    def patch(self, operations, path=None, source=None, mode=None, expect_sha=None):
        """Call patch and return the response.

        Either path or source (or both) must be provided.
        """
        params = {"operations": operations}
        if path is not None:
            params["path"] = path
        if source is not None:
            params["source"] = source
        if mode is not None:
            params["mode"] = mode
        if expect_sha is not None:
            params["expectSha"] = expect_sha
        return self.call("patch", params)

    def explain(self, code):
        """Call explain and return the response."""
        return self.call("explain", {"code": code})

    def skills_get(self, name=None):
        """Call skillsGet and return the response."""
        params = {}
        if name is not None:
            params["name"] = name
        return self.call("skillsGet", params)


def main():
    """CLI for testing and demonstration purposes."""
    import argparse

    parser = argparse.ArgumentParser(
        description="Reference client for miri agent protocol"
    )
    parser.add_argument("--miri", required=True, help="path to miri binary")
    parser.add_argument(
        "mode",
        choices=["demo", "full"],
        help="mode of operation",
    )
    parser.add_argument(
        "file",
        help="source file for demo",
    )

    args = parser.parse_args()

    if args.mode == "demo":
        with AgentSession(args.miri) as session:
            # Initialize
            init_result = session.initialize()

            # Check the file
            check_result = session.check(args.file)

            # View the file (outline)
            view_result = session.view(args.file)

            # Return a summary as JSON to stdout
            summary = {
                "ok": check_result.get("result", {}).get("ok", False),
                "view": view_result.get("result", {}).get("view", {}),
                "diagnostics_count": len(
                    check_result.get("result", {}).get("diagnostics", [])
                ),
            }

            print(json.dumps(summary))

    elif args.mode == "full":
        with AgentSession(args.miri) as session:
            # Read the source for in-memory examples
            source_code = Path(args.file).read_text()

            # Exercise all public methods
            init_result = session.initialize()
            check_result = session.check(path=args.file)
            check_verify_result = session.check(path=args.file, verify_mir=True)
            check_source_result = session.check(source=source_code)
            check_source_with_path_result = session.check(path=args.file, source=source_code)
            view_result = session.view(args.file)
            view_fn_result = session.view(args.file, fn="main")
            explain_result = session.explain("MER_TYP_030")
            patch_result = session.patch([], path=args.file)
            patch_source_result = session.patch([], source=source_code, mode="checkOnly")
            skills_result = session.skills_get()
            skills_named_result = session.skills_get(name="miri-lang")

            # Return results as JSON to stdout
            summary = {
                "initialize": init_result.get("result") is not None,
                "check": check_result.get("result") is not None,
                "check_with_verify": check_verify_result.get("result") is not None,
                "check_with_source": check_source_result.get("result") is not None,
                "check_with_source_and_path": check_source_with_path_result.get("result") is not None,
                "view": view_result.get("result") is not None,
                "view_with_fn": view_fn_result.get("result") is not None,
                "explain": explain_result.get("result") is not None,
                "patch": patch_result.get("result") is not None,
                "patch_with_source": patch_source_result.get("result") is not None,
                "skills_get": skills_result.get("result") is not None,
                "skills_get_named": skills_named_result.get("result") is not None,
            }

            print(json.dumps(summary))


if __name__ == "__main__":
    main()
