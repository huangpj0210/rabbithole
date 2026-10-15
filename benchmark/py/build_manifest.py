#!/usr/bin/env python3
"""rabbithole benchmark —— 构建清单生成(goal-repair 证据 schema 的 build/)。

在**构建被测二进制的机器**上、构建完成后立即运行:

    python3 benchmark/py/build_manifest.py <repo根> <二进制目录> [输出目录]

  例:python3 benchmark/py/build_manifest.py . target/release /tmp/rh-build

产物(拷入每个证据目录的 build/ 下,verify_goal 会逐项核对):
  sources.sha256   参与构建的源文件 SHA-256(sha256sum -c 兼容格式)
  binaries.json    rh-server / rh-client 的 SHA-256 与绝对路径、生成时间、repo_root
  bin/rh-server, bin/rh-client   二进制副本(存在即被复核)

★r3:源文件集合由 py/source_inventory.py 的确定性枚举规则给出
(build_manifest 与 verify_goal 共享同一实现)——清单必须覆盖
Cargo.toml/Cargo.lock、crates/**、vendor/** 的全部参与文件;校验方会
对照同一枚举规则查遗漏/多余/重复,并拒绝符号链接与路径逃逸。

verify_goal 的核对口径:
  * RH_VERIFY_REPO(实测验收固定为最终工作区)优先,binaries.json.repo_root
    仅作回退——证据不能自带伪 repo 逃避最终工作区绑定;
  * 端点 /proc/PID/exe SHA-256 必须等于 binaries.json(运行的就是这份构建);
  * build/bin/ 存在时重算哈希必须一致;跨目录 manifest 必须完全一致。
"""
from __future__ import annotations

import datetime
import hashlib
import json
import os
import shutil
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import source_inventory  # noqa: E402 (同目录共享模块)


def sha256_file(path: str) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def main() -> int:
    if len(sys.argv) < 3:
        print(__doc__)
        return 2
    repo, bin_dir = sys.argv[1], sys.argv[2]
    out_dir = sys.argv[3] if len(sys.argv) > 3 else os.path.join(os.getcwd(), "build-manifest")
    if not os.path.isdir(repo):
        print(f"不是目录: {repo}", file=sys.stderr)
        return 1
    if not source_inventory.looks_like_cargo_repo(repo):
        print(f"{repo} 不像 Cargo 工作区(未找到任何 Cargo.toml)", file=sys.stderr)
        return 1
    missing = [b for b in ("rh-server", "rh-client")
               if not os.path.isfile(os.path.join(bin_dir, b))]
    if missing:
        print(f"{bin_dir} 缺少二进制: {missing}", file=sys.stderr)
        return 1

    os.makedirs(out_dir, exist_ok=True)
    srcs = source_inventory.enumerate_source_files(repo)
    if not srcs:
        print("枚举结果为空(仓库根不对?)", file=sys.stderr)
        return 1
    with open(os.path.join(out_dir, "sources.sha256"), "w", encoding="utf-8") as fh:
        for rel in srcs:
            fh.write(f"{sha256_file(os.path.join(repo, rel))}  {rel}\n")

    binaries = {
        "created_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "repo_root": os.path.abspath(repo),
        "enumerated_files": len(srcs),
        "binaries": {},
    }
    for b in ("rh-server", "rh-client"):
        p = os.path.join(bin_dir, b)
        binaries["binaries"][b] = {"sha256": sha256_file(p), "path": os.path.abspath(p)}
    with open(os.path.join(out_dir, "binaries.json"), "w", encoding="utf-8") as fh:
        json.dump(binaries, fh, ensure_ascii=False, indent=2)

    bin_dst = os.path.join(out_dir, "bin")
    os.makedirs(bin_dst, exist_ok=True)
    for b in ("rh-server", "rh-client"):
        shutil.copy2(os.path.join(bin_dir, b), os.path.join(bin_dst, b))

    print(f"build 清单已写入 {out_dir}:")
    print(f"  sources.sha256   {len(srcs)} 个源文件(枚举规则:source_inventory.py)")
    for b, meta in binaries["binaries"].items():
        print(f"  {b}: {meta['sha256']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
