# W0 构建前后使用同一算法核对源码身份；agents 内的构建日志不参与。
import hashlib, pathlib, subprocess
root = pathlib.Path.cwd()
files = subprocess.check_output(["git", "ls-files", "-z"]).split(b"\0")
digest = hashlib.sha256()
count = 0
for raw in sorted(files):
    if not raw or raw.startswith(b"agents/"):
        continue
    path = root / raw.decode()
    digest.update(raw + b"\0" + hashlib.sha256(path.read_bytes()).digest())
    count += 1
print("commit=" + subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip())
print("tracked_source_files=" + str(count))
print("source_sha256=" + digest.hexdigest())
print("license_sha256=" + hashlib.sha256((root / "LICENSE").read_bytes()).hexdigest())
