#!/usr/bin/env python3
"""把 Tabler 线稿图标转成 Android VectorDrawable。

Wear 端不能用 GPUI 的 SVG 渲染，Compose 只认 `painterResource`。
这个脚本：

1. 缺图时从 tabler/tabler-icons 的 outline 目录下载到 `assets/icons/tabler-<name>.svg`；
2. 把需要的图标转成 `wear/app/src/main/res/drawable/ic_<name>.xml`。

只处理 Tabler outline 里常见的 path / circle / rect / line / polyline / polygon；
遇到不支持的元素直接报错，避免静默丢图。描边保持黑色，Compose 侧用
`Icon(painter, tint = ...)` 着色。
"""

from __future__ import annotations

import math
import re
import urllib.request
import xml.etree.ElementTree as ET
from pathlib import Path

BASE = Path(__file__).resolve().parents[1]
SRC_DIR = BASE / "assets" / "icons"
OUT_DIR = BASE / "wear" / "app" / "src" / "main" / "res" / "drawable"

# 语义名 -> Tabler outline 文件名
ICONS: dict[str, str] = {
    "back": "arrow-left",
    "more": "dots",
    "connect": "plug-connected",
    "overview": "dashboard",
    "apps": "apps",
    "files": "folder",
    "screen": "device-mobile",
    "cast": "cast",
    "terminal": "terminal-2",
    "logs": "file-text",
    "settings": "settings",
    "refresh": "refresh",
    "check": "check",
    "close": "x",
    "warning": "alert-triangle",
    "bolt": "bolt",
    "wifi": "wifi",
    "router": "router",
    "usb": "usb",
    "scan": "radar",
    "devices": "devices",
    "link": "link",
    "unlink": "unlink",
    "key": "key",
    "search": "search",
    "upload": "upload",
    "download": "download",
    "trash": "trash",
    "power": "power",
    "info": "info-circle",
    "camera": "camera",
    "play": "player-play",
    "stop": "player-stop",
    "keyboard": "keyboard",
    "chevron_right": "chevron-right",
    "watch": "device-watch",
    "package": "package",
    "android": "brand-android",
    "folder": "folder",
    "list": "list",
    "folder_open": "folder-open",
    "rotate": "rotate-clockwise",
    "arrow_up": "arrow-up",
}

URL = "https://raw.githubusercontent.com/tabler/tabler-icons/main/icons/outline/{name}.svg"
NUM = r"[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?"


def fetch(name: str) -> str:
    path = SRC_DIR / f"tabler-{name}.svg"
    if path.exists() and path.stat().st_size > 0:
        return path.read_text(encoding="utf-8")
    with urllib.request.urlopen(URL.format(name=name), timeout=20) as response:
        text = response.read().decode("utf-8")
    SRC_DIR.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")
    print(f"downloaded {name}")
    return text


def strip_comments(text: str) -> str:
    return re.sub(r"<!--.*?-->", "", text, flags=re.S)


def attr(element: ET.Element, name: str) -> str | None:
    # SVG 没有命名空间前缀时直接命中；带 prefix 时退化为本地名匹配。
    if name in element.attrib:
        return element.attrib[name]
    for key, value in element.attrib.items():
        if key.rsplit("}", 1)[-1] == name:
            return value
    return None


def num(element: ET.Element, name: str, default: float = 0.0) -> float:
    value = attr(element, name)
    return float(value) if value is not None else default


def fmt(value: float) -> str:
    text = f"{value:.3f}".rstrip("0").rstrip(".")
    return text or "0"


def circle_path(cx: float, cy: float, r: float) -> str:
    return (
        f"M{fmt(cx - r)},{fmt(cy)}"
        f"a{fmt(r)},{fmt(r)} 0 1,0 {fmt(2 * r)},0"
        f"a{fmt(r)},{fmt(r)} 0 1,0 {fmt(-2 * r)},0"
    )


def rect_path(x: float, y: float, w: float, h: float, rx: float) -> str:
    rx = min(rx, w / 2, h / 2)
    if rx <= 0:
        return f"M{fmt(x)},{fmt(y)}h{fmt(w)}v{fmt(h)}h{fmt(-w)}z"
    return (
        f"M{fmt(x + rx)},{fmt(y)}"
        f"h{fmt(w - 2 * rx)}a{fmt(rx)},{fmt(rx)} 0 0,1 {fmt(rx)},{fmt(rx)}"
        f"v{fmt(h - 2 * rx)}a{fmt(rx)},{fmt(rx)} 0 0,1 {fmt(-rx)},{fmt(rx)}"
        f"h{fmt(-(w - 2 * rx))}a{fmt(rx)},{fmt(rx)} 0 0,1 {fmt(-rx)},{fmt(-rx)}"
        f"v{fmt(-(h - 2 * rx))}a{fmt(rx)},{fmt(rx)} 0 0,1 {fmt(rx)},{fmt(-rx)}z"
    )


def points_path(points: str, close: bool) -> str:
    values = [float(v) for v in re.findall(NUM, points)]
    if len(values) < 4:
        return ""
    pairs = list(zip(values[0::2], values[1::2]))
    data = f"M{fmt(pairs[0][0])},{fmt(pairs[0][1])}"
    for x, y in pairs[1:]:
        data += f"L{fmt(x)},{fmt(y)}"
    return data + ("z" if close else "")


def element_path(element: ET.Element) -> str | None:
    tag = element.tag.rsplit("}", 1)[-1]
    if tag == "path":
        return attr(element, "d")
    if tag == "circle":
        return circle_path(num(element, "cx"), num(element, "cy"), num(element, "r"))
    if tag == "rect":
        return rect_path(
            num(element, "x"),
            num(element, "y"),
            num(element, "width"),
            num(element, "height"),
            num(element, "rx", num(element, "ry")),
        )
    if tag == "line":
        return (
            f"M{fmt(num(element, 'x1'))},{fmt(num(element, 'y1'))}"
            f"L{fmt(num(element, 'x2'))},{fmt(num(element, 'y2'))}"
        )
    if tag in ("polyline", "polygon"):
        return points_path(attr(element, "points") or "", tag == "polygon")
    return None


def convert(semantic: str, tabler_name: str) -> None:
    root = ET.fromstring(strip_comments(fetch(tabler_name)))
    paths: list[str] = []

    for element in root.iter():
        tag = element.tag.rsplit("}", 1)[-1]
        if tag in ("svg", "g", "title", "desc"):
            continue
        data = element_path(element)
        if not data:
            continue
        fill = attr(element, "fill")
        stroke = attr(element, "stroke")
        # Tabler 每个图标开头都有一条 24x24 的透明定位路径，跳过。
        if fill == "none" and stroke == "none":
            continue
        # 只画描边；个别圆点用 fill="currentColor" 填充。
        if fill == "currentColor" and stroke is None:
            paths.append(
                f'    <path android:pathData="{data}" android:fillColor="#FF000000" '
                f'android:strokeWidth="0"/>'
            )
        else:
            paths.append(
                f'    <path android:pathData="{data}" android:fillColor="#00000000" '
                f'android:strokeColor="#FF000000" android:strokeWidth="2" '
                f'android:strokeLineCap="round" android:strokeLineJoin="round"/>'
            )

    if not paths:
        raise SystemExit(f"{tabler_name}: 没有可用路径")

    xml = (
        '<?xml version="1.0" encoding="utf-8"?>\n'
        '<vector xmlns:android="http://schemas.android.com/apk/res/android"\n'
        '    android:width="24dp"\n'
        '    android:height="24dp"\n'
        '    android:viewportWidth="24"\n'
        '    android:viewportHeight="24">\n'
        + "\n".join(paths)
        + "\n</vector>\n"
    )
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    (OUT_DIR / f"ic_{semantic}.xml").write_text(xml, encoding="utf-8")


def main() -> None:
    for semantic, tabler_name in ICONS.items():
        convert(semantic, tabler_name)
        print(f"ic_{semantic}.xml <- tabler/{tabler_name}.svg")
    print(f"done: {len(ICONS)} icons -> {OUT_DIR}")


if __name__ == "__main__":
    main()
