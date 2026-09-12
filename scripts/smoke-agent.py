"""Exercise a disposable Linux agent container; no cloud account or published ports.

Usage: python scripts/smoke-agent.py vsm-agent-smoke
The caller owns container creation and cleanup. Game files download from the official CDN.
"""
import json
import subprocess
import sys
import time

container = sys.argv[1]


def api(method, path, body=None):
    command = [
        "docker", "exec", "-i", container, "sh", "-c",
        'curl --fail-with-body -sS --max-time 300 -X "$1" '
        '-H "Authorization: Bearer $(cat /var/lib/vsm/agent.token)" '
        '-H "Content-Type: application/json" --data-binary @- '
        '"http://127.0.0.1:4318$2"', "sh", method, "/v1/" + path,
    ]
    result = subprocess.run(command, input=json.dumps(body or {}), text=True, capture_output=True, timeout=310)
    if result.returncode:
        raise RuntimeError(f"{path}: {result.stdout} {result.stderr}")
    return json.loads(result.stdout)


print("Installing official Vintage Story 1.22.0", flush=True)
assert api("POST", "actions/install", {"version": "1.22.0"})["installed"] == "1.22.0"
print("Starting server", flush=True)
api("POST", "actions/start")
for attempt in range(90):
    logs = api("GET", "logs")
    status = api("GET", "status")
    if status["state"] == "stopped":
        raise RuntimeError("Server exited: " + json.dumps(logs[-12:]))
    ready = subprocess.run(["docker", "exec", container, "bash", "-c", "true > /dev/tcp/127.0.0.1/42420"], capture_output=True).returncode == 0
    if ready and status["state"] == "running":
        break
    time.sleep(2)
else:
    raise RuntimeError("Game listener not ready: " + json.dumps(logs[-12:]))
assert subprocess.run(["docker", "exec", "--user", "vintagestory", container, "test", "-r", "/var/lib/vsm/agent.token"]).returncode != 0, "Game account can read the agent token"
print("Game TCP listener is reachable; exercising console and backup", flush=True)
api("POST", "actions/console", {"command": "stats"})
backup = api("POST", "actions/backup")
points = api("GET", "backups")["recovery_points"]
assert points
api("POST", "actions/stop")
assert api("GET", "status")["state"] == "stopped"
api("POST", "actions/restore", {"id": points[-1]["name"]})
assert api("GET", "status")["game_version"] == "1.22.0"
print("Exercising encrypted restic repository and snapshot restore", flush=True)
# HOSTNAME is a disposable-container-only password fixture, never a production recommendation.
policy = {"enabled": False, "repository": "/tmp/vsm-smoke-restic", "password_env": "HOSTNAME", "hour_utc": (time.gmtime().tm_hour + 2) % 24, "daily": 7, "weekly": 4}
api("PUT", "backup-policy", policy)
try:
    api("POST", "actions/backup-init")
except RuntimeError:
    api("POST", "actions/backup-check")
policy["enabled"] = True
api("PUT", "backup-policy", policy)
before = api("POST", "actions/backup-snapshots")
identity = subprocess.check_output(["docker", "exec", container, "cat", "/var/lib/vsm/identity"], text=True).strip()
other_ids = {s["id"] for s in before if "vsm:" + identity not in s.get("tags", [])}
assert api("POST", "actions/backup")["off_host"] is True
assert other_ids.issubset({s["id"] for s in api("POST", "actions/backup-snapshots")}), "Retention removed another source's snapshot"
snapshots = api("POST", "actions/backup-snapshots")
assert snapshots
api("POST", "actions/backup-check")
api("POST", "actions/backup-restore", {"snapshot": snapshots[-1]["id"]})
assert api("GET", "status")["game_version"] == "1.22.0"
policy["enabled"] = False
api("PUT", "backup-policy", policy)
print(json.dumps({"result": "passed", "version": "1.22.0", "checks": ["official installation", "game TCP listener", "console command submission", "consistent backup", "graceful stop", "local restore", "game account cannot read agent token", "encrypted restic backup/check/restore"]}, indent=2))
