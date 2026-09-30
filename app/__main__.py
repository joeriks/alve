"""Run with: python -m app --data-dir ./private-vaults/poc"""
import argparse
import os
from pathlib import Path

from .server import AlveServer
from .vault import Vault


def main():
    parser = argparse.ArgumentParser(description="Alve local memory proof of concept")
    parser.add_argument("--port", type=int, default=4765)
    parser.add_argument("--data-dir", type=Path, default=Path("private-vaults/poc"))
    args = parser.parse_args()
    args.data_dir.mkdir(parents=True, exist_ok=True)
    # Prevent two processes from silently overwriting the same snapshot.
    lock = (args.data_dir / ".process-lock").open("a+b")
    if lock.tell() == 0:
        lock.write(b"0")
        lock.flush()
    lock.seek(0)
    try:
        if os.name == "nt":
            import msvcrt
            msvcrt.locking(lock.fileno(), msvcrt.LK_NBLCK, 1)
        else:
            import fcntl
            fcntl.flock(lock.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
    except OSError:
        raise SystemExit("Another Alve process already uses this data directory.") from None
    server = AlveServer(("127.0.0.1", args.port), Vault(args.data_dir / "memory.alve"))
    print(f"Alve POC: http://127.0.0.1:{server.server_port}", flush=True)
    print("Loopback only. Encrypted snapshots; manual encrypted bundle exchange.", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
        with server.vault.mutex:
            server.vault.lock()
        lock.close()


if __name__ == "__main__":
    main()
