import init, * as wasm from "./rarebox_web.js";

const $ = (selector, root = document) => root.querySelector(selector);
const $$ = (selector, root = document) => [...root.querySelectorAll(selector)];

const RELEASES_URL = "https://github.com/Genouka/rarebox_desktop_next/releases";

const state = {
  view: "connect",
  connected: false,
  info: null,
  busy: false,
  snapshot: null,
  packages: [],
  packagesFor: null,
  filesPath: "/sdcard",
  files: [],
  logRunning: false,
  logPromise: null,
  logLines: [],
  logBuffer: "",
  logFilter: "",
  logDirty: false,
  logRaf: null,
  screenshotUrl: null,
  history: [],
  historyIndex: 0,
};

const PAGE_TITLES = {
  connect: "连接",
  overview: "概览",
  apps: "应用",
  files: "文件",
  terminal: "终端",
  logs: "日志",
  screen: "屏幕",
  about: "客户端",
};

/* ── 基础工具 ─────────────────────────────────────────── */

function esc(value) {
  return String(value ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

function errText(error) {
  if (!error) return "未知错误";
  if (typeof error === "string") return error;
  if (error.message) return error.message;
  try {
    return String(error);
  } catch {
    return "未知错误";
  }
}

function toast(message, kind = "info", timeout = 3600) {
  const node = document.createElement("div");
  node.className = `toast ${kind === "info" ? "" : kind}`;
  node.textContent = message;
  $("#toasts").appendChild(node);
  setTimeout(() => {
    node.style.opacity = "0";
    node.style.transition = "opacity .2s";
    setTimeout(() => node.remove(), 220);
  }, timeout);
}

function setBusy(value, label = "正在处理…") {
  state.busy = value;
  $("#busy-label").textContent = label;
  $("#busy").classList.toggle("hidden", !value);
}

async function run(label, work) {
  if (state.busy) {
    toast("请等待当前操作完成", "info");
    return undefined;
  }
  if (state.logRunning) {
    await stopLogs();
  }
  setBusy(true, label);
  try {
    return await work();
  } catch (error) {
    let message = errText(error);
    if (/no device selected/i.test(message)) {
      message = "已取消选择 USB 设备";
    }
    toast(message, "error", 6000);
    return undefined;
  } finally {
    setBusy(false);
  }
}

function downloadBytes(bytes, name, type) {
  const blob = new Blob([bytes], { type });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = name;
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  setTimeout(() => URL.revokeObjectURL(url), 2000);
}

function formatFileTime(seconds) {
  if (!seconds) return "—";
  return new Date(seconds * 1000).toLocaleString("zh-CN", { hour12: false });
}

/* ── 弹窗 ─────────────────────────────────────────────── */

let modalCleanup = null;

function closeModal() {
  $("#modal").classList.add("hidden");
  if (modalCleanup) {
    modalCleanup();
    modalCleanup = null;
  }
}

function openModal({ title, body, actions = [], dismissible = true }) {
  closeModal();
  $("#modal-title").textContent = title;
  const bodyNode = $("#modal-body");
  bodyNode.replaceChildren();
  if (typeof body === "string") {
    bodyNode.innerHTML = body;
  } else if (body) {
    bodyNode.appendChild(body);
  }
  const actionsNode = $("#modal-actions");
  actionsNode.replaceChildren();
  for (const action of actions) {
    const button = document.createElement("button");
    button.className = `button ${action.kind || "text"}`;
    button.textContent = action.label;
    button.addEventListener("click", async () => {
      if (action.onClick) {
        const keep = await action.onClick();
        if (keep === true) return;
      }
      closeModal();
    });
    actionsNode.appendChild(button);
  }
  $("#modal").classList.remove("hidden");

  const backdrop = $("#modal");
  const onBackdrop = (event) => {
    if (event.target === backdrop && dismissible) closeModal();
  };
  const onKey = (event) => {
    if (event.key === "Escape" && dismissible) closeModal();
  };
  backdrop.addEventListener("click", onBackdrop);
  document.addEventListener("keydown", onKey);
  modalCleanup = () => {
    backdrop.removeEventListener("click", onBackdrop);
    document.removeEventListener("keydown", onKey);
  };
}

function confirmAction(title, message, confirmLabel = "确定", danger = true) {
  return new Promise((resolve) => {
    let decided = false;
    openModal({
      title,
      body: `<p>${esc(message)}</p>`,
      actions: [
        { label: "取消", kind: "text", onClick: () => { decided = true; resolve(false); } },
        {
          label: confirmLabel,
          kind: danger ? "danger" : "filled",
          onClick: () => { decided = true; resolve(true); },
        },
      ],
    });
    modalCleanup = ((previous) => () => {
      previous?.();
      if (!decided) resolve(false);
    })(modalCleanup);
  });
}

function promptText(title, label, placeholder = "", value = "") {
  return new Promise((resolve) => {
    let decided = false;
    const wrapper = document.createElement("div");
    wrapper.innerHTML = `<label class="muted small">${esc(label)}</label><input class="input" style="width:100%;margin-top:6px" />`;
    const input = $("input", wrapper);
    input.placeholder = placeholder;
    input.value = value;
    openModal({
      title,
      body: wrapper,
      actions: [
        { label: "取消", kind: "text", onClick: () => { decided = true; resolve(null); } },
        {
          label: "确定",
          kind: "filled",
          onClick: () => { decided = true; resolve(input.value.trim()); },
        },
      ],
    });
    setTimeout(() => input.focus(), 30);
    input.addEventListener("keydown", (event) => {
      if (event.key === "Enter") {
        decided = true;
        resolve(input.value.trim());
        closeModal();
      }
    });
    modalCleanup = ((previous) => () => {
      previous?.();
      if (!decided) resolve(null);
    })(modalCleanup);
  });
}

function showWirelessUnsupported() {
  if (!$("#modal").classList.contains("hidden")) return;
  openModal({
    title: "浏览器版不支持无线调试",
    body: `
      <p>浏览器只能通过 <strong>WebUSB</strong> 访问 USB 设备，无法连接设备的局域网端口，
      因此不支持无线调试、无线配对和 TCP/IP 切换。</p>
      <p>需要无线调试、HarmonyOS hdc 或实时投屏时，请下载桌面客户端：</p>
      <div class="button-row">
        <a class="button tonal" href="${RELEASES_URL}" target="_blank" rel="noreferrer">下载桌面客户端</a>
      </div>
      <p class="small">当前页面仍可通过 USB 数据线正常调试。</p>
    `,
    actions: [{ label: "继续使用 USB", kind: "filled" }],
  });
}

/* ── 视图切换 ─────────────────────────────────────────── */

function setView(view) {
  if (view !== "connect" && view !== "about" && !state.connected) {
    toast("请先通过 USB 连接设备", "info");
    view = "connect";
  }
  if (state.logRunning && view !== "logs") {
    stopLogs();
  }
  state.view = view;
  $$(".view").forEach((node) => {
    node.classList.toggle("hidden", node.dataset.view !== view);
  });
  $$(".nav-item[data-view]").forEach((node) => {
    node.classList.toggle("active", node.dataset.view === view);
  });
  $("#page-title").textContent = PAGE_TITLES[view] || "RareBox";
  $(".sidebar").classList.remove("open");
  if (view === "overview") loadOverview();
  if (view === "apps" && state.packagesFor !== state.info?.serial) loadApps();
  if (view === "files" && !state.files.length) loadFiles(state.filesPath);
}

function setConnected(value) {
  state.connected = value;
  $$(".nav-item[data-needs-device]").forEach((node) => {
    node.disabled = !value;
  });
  $("#device-pill").classList.toggle("hidden", !value);
  if (!value) {
    state.info = null;
    state.snapshot = null;
    state.packages = [];
    state.packagesFor = null;
    state.files = [];
    $("#device-pill-text").textContent = "";
    setView("connect");
  }
}

function updateDevicePill() {
  if (!state.info) return;
  const name = state.info.model || state.info.usbProductName || state.info.serial;
  $("#device-pill-text").textContent = name;
  $("#device-pill").title = `${state.info.serial} · 点击查看设备信息`;
}

/* ── 连接页 ───────────────────────────────────────────── */

function updateUsbWarning() {
  const warning = $("#usb-warning");
  const messages = [];
  if (!window.isSecureContext) {
    messages.push("WebUSB 需要 HTTPS 或 localhost。当前页面不是安全上下文，浏览器会拒绝访问 USB。");
  }
  if (!wasm.usb_supported()) {
    messages.push("当前浏览器不支持 WebUSB。请使用 Chrome / Edge 89+ 的桌面版。");
  }
  if (messages.length) {
    warning.textContent = messages.join(" ");
    warning.classList.remove("hidden");
  } else {
    warning.classList.add("hidden");
  }
}

async function refreshAuthorized() {
  const list = $("#usb-devices");
  if (!wasm.usb_supported()) {
    list.innerHTML = `<p class="muted">此浏览器不支持 WebUSB。</p>`;
    return;
  }
  let devices = [];
  try {
    devices = JSON.parse(await wasm.list_authorized_devices());
  } catch (error) {
    list.innerHTML = `<p class="muted">读取 USB 设备失败：${esc(errText(error))}</p>`;
    return;
  }
  if (!devices.length) {
    list.innerHTML = `<p class="muted">尚未检测到已授权的 USB 设备。点击上方「选择 USB 设备」开始。</p>`;
    return;
  }
  list.innerHTML = devices
    .map((device) => {
      const name = device.productName || `${device.vendorId.toString(16)}:${device.productId.toString(16)}`;
      const meta = [
        device.manufacturerName,
        `VID ${device.vendorId.toString(16).padStart(4, "0")} · PID ${device.productId
          .toString(16)
          .padStart(4, "0")}`,
        device.serialNumber,
      ]
        .filter(Boolean)
        .join(" · ");
      const disabled = device.hasAdbInterface ? "" : "disabled";
      const tag = device.hasAdbInterface
        ? `<span class="tag ok">ADB 接口</span>`
        : `<span class="tag warn">未找到 ADB 接口</span>`;
      return `
        <div class="device-item">
          <span class="icon" style="--icon: url('icons/smartphone.svg')"></span>
          <div class="device-meta">
            <strong>${esc(name)}</strong>
            <span>${esc(meta)}</span>
          </div>
          ${tag}
          <button class="button tonal" data-usb-connect="${device.index}" ${disabled}>连接</button>
        </div>`;
    })
    .join("");
}

async function connectByIndex(index) {
  const json = await run("正在连接 USB 设备…", () => wasm.connect_authorized(index));
  if (json) onConnected(json);
}

async function connectNewDevice() {
  try {
    const json = await run("正在等待选择 USB 设备…", () => wasm.request_and_connect());
    if (json) onConnected(json);
  } finally {
    await refreshAuthorized();
  }
}

function onConnected(json) {
  const parsed = JSON.parse(json);
  state.info = parsed.info;
  setConnected(true);
  updateDevicePill();
  toast(`已连接 ${state.info.model || state.info.serial}`, "success");
  refreshAuthorized();
  setView("overview");
}

async function disconnectDevice() {
  await run("正在断开连接…", () => wasm.disconnect());
  setConnected(false);
  refreshAuthorized();
  toast("已断开 USB 连接", "info");
}

/* ── 概览 ─────────────────────────────────────────────── */

function statCard(icon, label, value, sub, percent = null) {
  const bar =
    percent === null
      ? ""
      : `<div class="bar"><span style="width:${Math.min(100, Math.max(0, percent))}%"></span></div>`;
  return `
    <article class="card stat">
      <div class="label"><span class="icon" style="--icon: url('icons/${icon}.svg')"></span>${esc(label)}</div>
      <div class="value">${esc(value)}</div>
      ${sub ? `<div class="sub">${esc(sub)}</div>` : ""}
      ${bar}
    </article>`;
}

async function loadOverview() {
  if (!state.connected) return;
  const json = await run("正在读取设备信息…", () => wasm.snapshot());
  if (!json) return;
  const snapshot = JSON.parse(json);
  state.snapshot = snapshot;

  const tags = [
    snapshot.android ? `Android ${snapshot.android}` : null,
    snapshot.sdk ? `SDK ${snapshot.sdk}` : null,
    snapshot.abi || null,
    snapshot.wearable ? "Wear OS" : null,
  ].filter(Boolean);

  $("#overview-hero").innerHTML = `
    <div class="device-icon"><span class="icon" style="--icon: url('icons/${snapshot.wearable ? "sensors" : "smartphone"}.svg")"></span></div>
    <div class="grow">
      <h2>${esc(snapshot.title)}</h2>
      <div class="meta">
        ${tags.map((tag) => `<span class="tag">${esc(tag)}</span>`).join("")}
        <span class="tag">${esc(snapshot.serial)}</span>
      </div>
      <div class="meta small muted">
        <span>${esc(snapshot.manufacturer || "—")}</span>
        ${snapshot.ip ? `<span>IP ${esc(snapshot.ip)}</span>` : ""}
        <span>负载 ${esc(snapshot.load || "—")}</span>
      </div>
    </div>
    <button class="button outlined" id="overview-refresh">
      <span class="icon" style="--icon: url('icons/refresh.svg')"></span>刷新
    </button>`;

  const batteryLevel = snapshot.battery.level ?? 0;
  const memoryPercent = snapshot.memory.percent ?? 0;
  const storagePercent = snapshot.storage.percent ?? 0;
  const displaySize = snapshot.display.overrideSize || snapshot.display.physicalSize || "—";
  const density = snapshot.display.overrideDensity || snapshot.display.physicalDensity || "—";

  $("#overview-grid").innerHTML = [
    statCard(
      "battery_full",
      "电量",
      snapshot.battery.level === null ? "—" : `${snapshot.battery.level}%`,
      `${snapshot.battery.statusLabel}${snapshot.battery.temperature !== null ? ` · ${snapshot.battery.temperature}°C` : ""}`,
      batteryLevel,
    ),
    statCard(
      "storage",
      "存储（/data）",
      snapshot.storage.percent ? `${snapshot.storage.percent}%` : "—",
      `${snapshot.storage.used || "—"} / ${snapshot.storage.size || "—"}`,
      storagePercent,
    ),
    statCard(
      "memory",
      "内存",
      snapshot.memory.totalKb ? `${snapshot.memory.percent}%` : "—",
      `${snapshot.memory.usedLabel} / ${snapshot.memory.totalLabel}`,
      memoryPercent,
    ),
    statCard("speed", "运行时间", snapshot.uptimeLabel || "—", snapshot.load ? `负载 ${snapshot.load}` : ""),
    statCard("aspect_ratio", "屏幕", displaySize, `DPI ${density} · 旋转 ${snapshot.display.rotationLabel}`),
    statCard(
      "info",
      "系统",
      snapshot.buildId || "—",
      [snapshot.patch ? `补丁 ${snapshot.patch}` : null, snapshot.characteristics].filter(Boolean).join(" · "),
    ),
  ].join("");

  $("#overview-refresh").addEventListener("click", loadOverview);
}

/* ── 应用 ─────────────────────────────────────────────── */

async function loadApps(force = false) {
  if (!state.connected) return;
  if (!force && state.packagesFor === state.info?.serial) {
    renderApps();
    return;
  }
  const json = await run("正在读取应用列表…", () => wasm.list_packages());
  if (!json) return;
  state.packages = JSON.parse(json);
  state.packagesFor = state.info?.serial;
  renderApps();
}

function renderApps() {
  const query = $("#apps-search").value.trim().toLowerCase();
  const scope = $("#apps-scope").value;
  const packages = state.packages.filter((pkg) => {
    if (scope === "user" && pkg.system) return false;
    if (scope === "system" && !pkg.system) return false;
    if (query && !pkg.name.toLowerCase().includes(query)) return false;
    return true;
  });

  const list = $("#apps-list");
  if (!packages.length) {
    list.innerHTML = `<div class="list-empty">${
      state.packages.length ? "没有匹配的应用" : "还没有加载应用列表"
    }</div>`;
    return;
  }

  list.innerHTML = packages
    .map((pkg, index) => {
      const version = pkg.versionCode ? `versionCode ${pkg.versionCode}` : "版本未知";
      return `
        <div class="list-row">
          <span class="icon" style="--icon: url('icons/apps.svg')"></span>
          <div class="list-main">
            <strong>${esc(pkg.name)}</strong>
            <span>${esc(version)}</span>
          </div>
          <span class="tag ${pkg.system ? "" : "ok"}">${pkg.system ? "系统" : "用户"}</span>
          <div class="row-actions">
            <button class="button text" data-app-action="launch" data-index="${index}">启动</button>
            <button class="button text" data-app-action="stop" data-index="${index}">停止</button>
            <button class="button text" data-app-action="export" data-index="${index}">导出</button>
            <button class="button text" data-app-action="uninstall" data-index="${index}">卸载</button>
          </div>
        </div>`;
    })
    .join("");

  list.dataset.filtered = JSON.stringify(packages.map((pkg) => pkg.name));
}

function filteredPackage(index) {
  const names = JSON.parse($("#apps-list").dataset.filtered || "[]");
  const name = names[index];
  return state.packages.find((pkg) => pkg.name === name);
}

async function onAppAction(action, index) {
  const pkg = filteredPackage(index);
  if (!pkg) return;
  if (action === "launch") {
    const output = await run(`正在启动 ${pkg.name}…`, () => wasm.launch_package(pkg.name));
    if (output !== undefined) toast(`已发送启动命令`, "success");
  } else if (action === "stop") {
    await run(`正在停止 ${pkg.name}…`, () => wasm.stop_package(pkg.name));
    toast("已强制停止", "success");
  } else if (action === "uninstall") {
    const ok = await confirmAction("卸载应用？", `将从设备卸载 ${pkg.name}。`, "卸载");
    if (!ok) return;
    const output = await run(`正在卸载 ${pkg.name}…`, () => wasm.uninstall_package(pkg.name));
    if (output) toast(output.trim(), "success");
  } else if (action === "export") {
    const bytes = await run(`正在导出 ${pkg.name}…`, () => wasm.export_package(pkg.name));
    if (bytes) {
      downloadBytes(bytes, `${pkg.name}.apk`, "application/vnd.android.package-archive");
      toast("APK 已开始下载", "success");
    }
  }
}

async function installSelectedApks() {
  const input = $("#apps-file");
  const files = [...input.files];
  input.value = "";
  if (!files.length) return;

  const total = files.reduce((sum, file) => sum + file.size, 0);
  const ok = await confirmAction(
    "安装 APK？",
    `${files.map((file) => file.name).join("、")}（共 ${(total / 1024 / 1024).toFixed(1)} MB）将上传到设备并安装。`,
    "安装",
    false,
  );
  if (!ok) return;

  const report = await run("正在上传并安装 APK…", async () => {
    const buffers = await Promise.all(files.map((file) => file.arrayBuffer()));
    const bundle = new Uint8Array(total);
    let offset = 0;
    for (const buffer of buffers) {
      bundle.set(new Uint8Array(buffer), offset);
      offset += buffer.byteLength;
    }
    const manifest = JSON.stringify({
      files: files.map((file, index) => ({ name: file.name, size: buffers[index].byteLength })),
    });
    const options = JSON.stringify({
      replace: $("#opt-replace").checked,
      downgrade: $("#opt-downgrade").checked,
      grantPermissions: $("#opt-grant").checked,
      allowTest: $("#opt-test").checked,
    });
    return wasm.install_apks(bundle, manifest, options);
  });
  if (!report) return;
  const parsed = JSON.parse(report);
  openModal({
    title: parsed.success ? "安装成功" : "安装失败",
    body: `<pre class="terminal-output" style="height:min(40vh,320px)">${esc(parsed.output || "（没有输出）")}</pre>`,
    actions: [{ label: "关闭", kind: "filled" }],
  });
  if (parsed.success) {
    toast("APK 安装完成", "success");
    loadApps(true);
  }
}

/* ── 文件 ─────────────────────────────────────────────── */

function normalizePath(path) {
  let value = (path || "").trim();
  if (!value) value = "/sdcard";
  if (!value.startsWith("/")) value = `/${value}`;
  while (value.length > 1 && value.endsWith("/")) value = value.slice(0, -1);
  return value;
}

async function loadFiles(path) {
  const target = normalizePath(path);
  state.filesPath = target;
  $("#files-path").value = target;
  const json = await run(`正在读取 ${target}…`, () => wasm.list_files(target));
  if (!json) return;
  state.files = JSON.parse(json);
  renderFiles();
}

function renderFiles() {
  const list = $("#files-list");
  if (!state.files.length) {
    list.innerHTML = `<div class="list-empty">空目录或无法读取</div>`;
    return;
  }
  list.innerHTML = state.files
    .map((entry, index) => {
      const icon = entry.isDir ? (entry.isLink ? "folder_off" : "folder") : "description";
      const meta = entry.isDir
        ? `文件夹 · ${esc(entry.permissions)}`
        : `${esc(entry.sizeLabel)} · ${formatFileTime(entry.mtime)}`;
      return `
        <div class="list-row">
          <span class="icon" style="--icon: url('icons/${icon}.svg')"></span>
          <div class="list-main">
            <strong>${esc(entry.name)}</strong>
            <span>${meta}</span>
          </div>
          <div class="row-actions">
            ${
              entry.isDir
                ? `<button class="button text" data-file-action="open" data-index="${index}">打开</button>`
                : `<button class="button text" data-file-action="download" data-index="${index}">下载</button>`
            }
            <button class="button text" data-file-action="delete" data-index="${index}">删除</button>
          </div>
        </div>`;
    })
    .join("");
}

function remotePathFor(entry) {
  return state.filesPath === "/" ? `/${entry.name}` : `${state.filesPath}/${entry.name}`;
}

async function onFileAction(action, index) {
  const entry = state.files[index];
  if (!entry) return;
  const remote = remotePathFor(entry);
  if (action === "open") {
    await loadFiles(remote);
  } else if (action === "download") {
    const bytes = await run(`正在下载 ${entry.name}…`, () => wasm.pull_file(remote));
    if (bytes) {
      downloadBytes(bytes, entry.name, "application/octet-stream");
      toast("已开始下载", "success");
    }
  } else if (action === "delete") {
    const ok = await confirmAction("删除？", `将删除设备上的 ${remote}，目录会连同内容一起删除。`, "删除");
    if (!ok) return;
    await run(`正在删除 ${entry.name}…`, () => wasm.delete_path(remote));
    toast("已删除", "success");
    loadFiles(state.filesPath);
  }
}

async function uploadFiles() {
  const input = $("#files-input");
  const files = [...input.files];
  input.value = "";
  if (!files.length) return;
  const result = await run(`正在上传 ${files.length} 个文件…`, async () => {
    let count = 0;
    for (const file of files) {
      const bytes = new Uint8Array(await file.arrayBuffer());
      await wasm.push_file(state.filesPath, file.name, bytes);
      count += 1;
    }
    return count;
  });
  if (result) {
    toast(`已上传 ${result} 个文件`, "success");
    loadFiles(state.filesPath);
  }
}

async function createDirectory() {
  const name = await promptText("新建文件夹", "文件夹名称");
  if (!name) return;
  const path = state.filesPath === "/" ? `/${name}` : `${state.filesPath}/${name}`;
  await run("正在创建文件夹…", () => wasm.make_directory(path));
  toast("文件夹已创建", "success");
  loadFiles(state.filesPath);
}

/* ── 终端 ─────────────────────────────────────────────── */

function appendTerminal(command, result) {
  const node = document.createElement("div");
  node.className = "terminal-entry";
  const exit = result.exitCode === null ? "" : `退出码 ${result.exitCode}`;
  const success = result.success ? "ok" : "err";
  node.innerHTML = `
    <div class="cmd">$ ${esc(command)}</div>
    <div class="${success}">${esc(result.combined || "（无输出）")}</div>
    ${exit ? `<div class="muted small">${esc(exit)}</div>` : ""}`;
  $("#terminal-output").appendChild(node);
  $("#terminal-output").scrollTop = $("#terminal-output").scrollHeight;
}

async function submitTerminal(command) {
  const value = command.trim();
  if (!value) return;
  state.history.push(value);
  state.historyIndex = state.history.length;
  const json = await run(`正在执行 ${value.slice(0, 36)}…`, () => wasm.terminal(value));
  if (!json) return;
  appendTerminal(value, JSON.parse(json));
}

/* ── 日志 ─────────────────────────────────────────────── */

function scheduleLogRender() {
  if (state.logRaf) return;
  state.logRaf = requestAnimationFrame(() => {
    state.logRaf = null;
    const buffer = state.logBuffer;
    state.logBuffer = "";
    if (!buffer) return;
    const lines = buffer.split(/\r?\n/);
    for (const line of lines) {
      if (!line) continue;
      state.logLines.push(line);
    }
    if (state.logLines.length > 6000) {
      state.logLines.splice(0, state.logLines.length - 6000);
    }
    renderLogs();
  });
}

function renderLogs() {
  const query = state.logFilter.trim().toLowerCase();
  const output = $("#logs-output");
  const lines = query
    ? state.logLines.filter((line) => line.toLowerCase().includes(query))
    : state.logLines;
  const fragment = document.createDocumentFragment();
  for (const line of lines) {
    const node = document.createElement("div");
    const level = /\s([VDIWEF])\s/.exec(line)?.[1] || "";
    node.className = `log-line ${level}`;
    node.textContent = line;
    fragment.appendChild(node);
  }
  output.replaceChildren(fragment);
  $("#logs-count").textContent = `${lines.length} 行`;
  if ($("#logs-follow").checked) {
    output.scrollTop = output.scrollHeight;
  }
}

function onLogChunk(text) {
  state.logBuffer += text;
  scheduleLogRender();
}

async function startLogs() {
  if (state.logRunning) return;
  state.logRunning = true;
  updateLogButton();
  state.logLines = [];
  state.logBuffer = "";
  renderLogs();
  try {
    state.logPromise = wasm
      .start_logcat($("#logs-level").value, onLogChunk)
      .catch((error) => {
        if (state.logRunning) toast(errText(error), "error");
      })
      .finally(() => {
        state.logRunning = false;
        state.logPromise = null;
        updateLogButton();
      });
  } catch (error) {
    state.logRunning = false;
    updateLogButton();
    toast(errText(error), "error");
  }
}

async function stopLogs() {
  if (!state.logRunning) return;
  wasm.stop_logcat();
  const promise = state.logPromise;
  state.logRunning = false;
  updateLogButton();
  if (promise) {
    await Promise.race([promise, new Promise((resolve) => setTimeout(resolve, 2500))]);
  }
}

function updateLogButton() {
  const button = $("#logs-toggle");
  button.textContent = state.logRunning ? "停止抓取" : "开始抓取";
  button.classList.toggle("filled", !state.logRunning);
  button.classList.toggle("danger", state.logRunning);
}

/* ── 屏幕 ─────────────────────────────────────────────── */

async function takeScreenshot() {
  const bytes = await run("正在截取屏幕…", () => wasm.screenshot());
  if (!bytes) return;
  if (state.screenshotUrl) URL.revokeObjectURL(state.screenshotUrl);
  state.screenshotUrl = URL.createObjectURL(new Blob([bytes], { type: "image/png" }));
  $("#screen-preview").innerHTML = `<img alt="设备截图" src="${state.screenshotUrl}" />`;
  const link = $("#screen-download");
  link.href = state.screenshotUrl;
  link.download = `rarebox-${Date.now()}.png`;
  link.classList.remove("hidden");
  toast("截图完成", "success");
}

async function recordScreen() {
  const bytes = await run("正在录制 10 秒屏幕…", () => wasm.record_screen(10));
  if (!bytes) return;
  downloadBytes(bytes, `rarebox-${Date.now()}.mp4`, "video/mp4");
  toast("录屏已开始下载", "success");
}

/* ── 事件绑定 ─────────────────────────────────────────── */

function wireEvents() {
  $$(".nav-item[data-view]").forEach((node) => {
    node.addEventListener("click", () => setView(node.dataset.view));
  });

  $("#menu-toggle").addEventListener("click", () => $(".sidebar").classList.toggle("open"));

  $("#connect-primary").addEventListener("click", connectNewDevice);
  $("#refresh-usb").addEventListener("click", () => {
    run("正在刷新 USB 设备…", refreshAuthorized);
  });
  $("#usb-devices").addEventListener("click", (event) => {
    const button = event.target.closest("[data-usb-connect]");
    if (button) connectByIndex(Number(button.dataset.usbConnect));
  });

  $$("[data-wireless]").forEach((node) => node.addEventListener("click", showWirelessUnsupported));
  $$("[data-wireless-field]").forEach((node) => {
    node.addEventListener("click", showWirelessUnsupported);
    node.addEventListener("focus", showWirelessUnsupported);
  });

  $("#device-pill").addEventListener("click", () => {
    if (!state.info) return;
    const info = state.info;
    openModal({
      title: info.model || info.usbProductName || "USB 设备",
      body: `
        <dl class="kv">
          <div><dt>序列号</dt><dd>${esc(info.serial)}</dd></div>
          <div><dt>产品</dt><dd>${esc(info.product || "—")}</dd></div>
          <div><dt>设备代号</dt><dd>${esc(info.device || "—")}</dd></div>
          <div><dt>USB</dt><dd>${info.vendorId.toString(16).padStart(4, "0")}:${info.productId
            .toString(16)
            .padStart(4, "0")}</dd></div>
          <div><dt>Shell 协议</dt><dd>${info.shellV2 ? "shell_v2（含退出码）" : "legacy"}</dd></div>
          <div><dt>连接方式</dt><dd>USB（WebUSB）</dd></div>
        </dl>`,
      actions: [
        { label: "断开连接", kind: "danger", onClick: async () => { await disconnectDevice(); } },
        { label: "关闭", kind: "text" },
      ],
    });
  });

  $("#theme-toggle").addEventListener("click", toggleTheme);

  $$("[data-reboot]").forEach((node) => {
    node.addEventListener("click", async () => {
      const target = node.dataset.reboot;
      const label = target === "system" ? "普通重启" : `重启到 ${target}`;
      const ok = await confirmAction("重启设备？", `将执行 ${label}，当前 USB 连接会中断。`, "重启");
      if (!ok) return;
      const output = await run(`正在${label}…`, () => wasm.reboot(target === "system" ? "" : target));
      if (output !== undefined) {
        toast("已发送重启命令", "success");
        setTimeout(() => {
          if (state.connected) {
            setConnected(false);
            toast("设备正在重启，请稍后重新连接", "info", 5000);
          }
        }, 2500);
      }
    });
  });

  $("#overview-refresh")?.addEventListener("click", loadOverview);

  // 应用页
  $("#apps-search").addEventListener("input", renderApps);
  $("#apps-scope").addEventListener("change", renderApps);
  $("#apps-refresh").addEventListener("click", () => loadApps(true));
  $("#apps-install").addEventListener("click", () => $("#apps-file").click());
  $("#apps-file").addEventListener("change", installSelectedApks);
  $("#apps-list").addEventListener("click", (event) => {
    const button = event.target.closest("[data-app-action]");
    if (button) onAppAction(button.dataset.appAction, Number(button.dataset.index));
  });

  // 文件页
  $("#files-go").addEventListener("click", () => loadFiles($("#files-path").value));
  $("#files-path").addEventListener("keydown", (event) => {
    if (event.key === "Enter") loadFiles($("#files-path").value);
  });
  $("#files-up").addEventListener("click", () => {
    const current = state.filesPath;
    const parent = current.slice(0, current.lastIndexOf("/")) || "/";
    loadFiles(parent);
  });
  $("#files-refresh").addEventListener("click", () => loadFiles(state.filesPath));
  $("#files-upload").addEventListener("click", () => $("#files-input").click());
  $("#files-input").addEventListener("change", uploadFiles);
  $("#files-mkdir").addEventListener("click", createDirectory);
  $("#files-list").addEventListener("click", (event) => {
    const button = event.target.closest("[data-file-action]");
    if (button) onFileAction(button.dataset.fileAction, Number(button.dataset.index));
  });

  // 终端
  $("#terminal-form").addEventListener("submit", (event) => {
    event.preventDefault();
    const input = $("#terminal-input");
    const value = input.value;
    input.value = "";
    submitTerminal(value);
  });
  $("#terminal-input").addEventListener("keydown", (event) => {
    if (event.key === "ArrowUp") {
      if (!state.history.length) return;
      state.historyIndex = Math.max(0, state.historyIndex - 1);
      $("#terminal-input").value = state.history[state.historyIndex] || "";
      event.preventDefault();
    } else if (event.key === "ArrowDown") {
      if (!state.history.length) return;
      state.historyIndex = Math.min(state.history.length, state.historyIndex + 1);
      $("#terminal-input").value = state.history[state.historyIndex] || "";
      event.preventDefault();
    }
  });
  $("#terminal-clear").addEventListener("click", () => $("#terminal-output").replaceChildren());
  $$(".quick .chip").forEach((chip) => {
    chip.addEventListener("click", () => submitTerminal(chip.dataset.cmd));
  });

  // 日志
  $("#logs-toggle").addEventListener("click", () => {
    if (state.logRunning) stopLogs();
    else startLogs();
  });
  $("#logs-filter").addEventListener("input", () => {
    state.logFilter = $("#logs-filter").value;
    renderLogs();
  });
  $("#logs-clear").addEventListener("click", async () => {
    const ok = await confirmAction("清空设备日志？", "将执行 logcat -c，并清空当前列表。", "清空");
    if (!ok) return;
    await run("正在清空日志…", () => wasm.clear_logs());
    state.logLines = [];
    state.logBuffer = "";
    renderLogs();
    toast("日志已清空", "success");
  });
  $("#logs-download").addEventListener("click", () => {
    if (!state.logLines.length) {
      toast("当前没有日志", "info");
      return;
    }
    downloadBytes(
      new Blob([state.logLines.join("\n")], { type: "text/plain;charset=utf-8" }),
      `rarebox-logcat-${Date.now()}.txt`,
      "text/plain;charset=utf-8",
    );
  });
  $("#logs-follow").addEventListener("change", renderLogs);

  // 屏幕
  $("#screen-shot").addEventListener("click", takeScreenshot);
  $("#screen-record").addEventListener("click", recordScreen);
  $$(".keys .chip").forEach((chip) => {
    chip.addEventListener("click", async () => {
      await run("正在发送按键…", () => wasm.send_key(Number(chip.dataset.key)));
    });
  });
  $("#text-form").addEventListener("submit", async (event) => {
    event.preventDefault();
    const input = $("#text-input");
    const value = input.value;
    if (!value.trim()) return;
    input.value = "";
    await run("正在发送文本…", () => wasm.send_text(value));
    toast("文本已发送", "success");
  });
  $("#display-size-apply").addEventListener("click", async () => {
    const match = /^(\d+)\s*[xX×]\s*(\d+)$/.exec($("#display-size").value.trim());
    if (!match) {
      toast("分辨率格式应为 宽x高，例如 450x450", "error");
      return;
    }
    await run("正在应用分辨率…", () => wasm.apply_display_size(Number(match[1]), Number(match[2])));
    loadOverview();
  });
  $("#display-size-reset").addEventListener("click", async () => {
    await run("正在重置分辨率…", () => wasm.reset_display_size());
    loadOverview();
  });
  $("#display-density-apply").addEventListener("click", async () => {
    const value = Number($("#display-density").value.trim());
    if (!Number.isFinite(value) || value <= 0) {
      toast("请输入有效的 DPI", "error");
      return;
    }
    await run("正在应用 DPI…", () => wasm.apply_density(value));
    loadOverview();
  });
  $("#display-density-reset").addEventListener("click", async () => {
    await run("正在重置 DPI…", () => wasm.reset_density());
    loadOverview();
  });
  $("#display-rotation-apply").addEventListener("click", async () => {
    await run("正在旋转屏幕…", () => wasm.set_rotation(Number($("#display-rotation").value)));
    loadOverview();
  });

  document.addEventListener("visibilitychange", () => {
    if (document.hidden && state.logRunning) stopLogs();
  });
}

/* ── 主题 ─────────────────────────────────────────────── */

function applyTheme(theme) {
  document.documentElement.dataset.theme = theme;
  localStorage.setItem("rarebox.theme", theme);
  const icon = $("#theme-icon");
  icon.style.setProperty(
    "--icon",
    `url('icons/${theme === "dark" ? "light_mode" : "dark_mode"}.svg')`,
  );
}

function toggleTheme() {
  const current = document.documentElement.dataset.theme;
  applyTheme(current === "dark" ? "light" : "dark");
}

/* ── 启动 ─────────────────────────────────────────────── */

async function boot() {
  const saved = localStorage.getItem("rarebox.theme");
  applyTheme(saved || (matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light"));

  try {
    await init();
  } catch (error) {
    $("#boot").innerHTML = `<p>无法加载 WebAssembly 模块：${esc(errText(error))}</p>`;
    return;
  }

  wireEvents();
  updateUsbWarning();
  setConnected(false);
  setView("connect");
  await refreshAuthorized();
  $("#boot").classList.add("hidden");
  $("#app").classList.remove("hidden");

  window.addEventListener("beforeunload", () => {
    if (state.logRunning) wasm.stop_logcat();
  });
}

boot();
