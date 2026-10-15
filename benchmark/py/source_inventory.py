#!/usr/bin/env python3
"""source_inventory —— 构建源清单的确定性枚举规则(build_manifest 与 verify_goal 共享)。

单一事实来源:py/build_manifest.py 产清单、py/verify_goal.py 验清单都必须用
同一个 enumerate_source_files(),否则"覆盖整个真实源集合"无从谈起。

规则(确定性,与语言/平台无关):
  * 根 Cargo.toml、Cargo.lock(存在即入);
  * crates/、vendor/ 下全部 *.rs、Cargo.toml、Cargo.lock;
  * 跳过符号链接(文件与目录)——逃逸/悬空链接不能作为可信构建输入,
    清单若列入符号链接条目,由校验方报错;
  * 返回相对路径排序列表。

校验方(verify_goal)据此做三件事:遗漏(repo 有、清单无)、多余(清单有、
枚举规则外)、重复(同一路径多条)全部 FAIL。
"""
from __future__ import annotations

import os

# 参与构建的源文件后缀/文件名(与 Cargo 构建输入一致)
_SOURCE_SUFFIXES = (".rs",)
_MANIFEST_NAMES = ("Cargo.toml", "Cargo.lock")
_SOURCE_ROOTS = ("crates", "vendor")


def _wanted(name: str) -> bool:
    return name.endswith(_SOURCE_SUFFIXES) or name in _MANIFEST_NAMES


def enumerate_source_files(repo: str) -> list[str]:
    """枚举 repo 中参与构建的源文件,返回相对路径排序列表(跳过符号链接)。"""
    out: list[str] = []
    for name in _MANIFEST_NAMES:
        p = os.path.join(repo, name)
        if os.path.isfile(p) and not os.path.islink(p):
            out.append(name)
    for root_dir in _SOURCE_ROOTS:
        top = os.path.join(repo, root_dir)
        if not os.path.isdir(top) or os.path.islink(top):
            continue
        for dirpath, dirnames, filenames in os.walk(top, followlinks=False):
            # 跳过符号链接目录
            dirnames[:] = sorted(d for d in dirnames
                                 if not os.path.islink(os.path.join(dirpath, d)))
            for name in sorted(filenames):
                if not _wanted(name):
                    continue
                p = os.path.join(dirpath, name)
                if os.path.islink(p):
                    continue
                out.append(os.path.relpath(p, repo))
    return sorted(set(out))


def looks_like_cargo_repo(repo: str) -> bool:
    """源树必须像个 Cargo 工作区(任意 Cargo.toml)——防伪 repo 逃避绑定。"""
    if os.path.isfile(os.path.join(repo, "Cargo.toml")):
        return True
    for root_dir in _SOURCE_ROOTS:
        top = os.path.join(repo, root_dir)
        if not os.path.isdir(top):
            continue
        for dirpath, dirnames, filenames in os.walk(top, followlinks=False):
            if "Cargo.toml" in filenames:
                return True
            dirnames[:] = sorted(d for d in dirnames
                                 if not os.path.islink(os.path.join(dirpath, d)))
    return False
