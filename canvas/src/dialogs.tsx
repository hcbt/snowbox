import { For, Show, createSignal, onSettled } from "solid-js";
import { Frame } from "./frame";
import {
  api,
  apiOn,
  attachHost,
  isJsonObject,
  type AgentOption,
  type AgentProgram,
  type EnvProgram,
  type EnvironmentDoc,
  type Json,
  type JsonObject,
  type Sandbox,
  type Template,
} from "./api";
import { overlayZ, type Overlay } from "./overlay";
import type { HostRec } from "./hosts";

const field =
  "mt-0.5 w-full box-border border border-twm-line bg-night-raised px-1 py-0.5 font-mono text-[13px] text-night-text";
const label = "mt-1.5 block font-medium";
const pickItem =
  "block w-full px-1 py-0.5 text-left font-mono text-[13px] hover:bg-twm-hi hover:text-white";
const push = "border border-twm-line bg-twm px-3 py-0.5 font-medium text-white active:scale-[0.97]";

type PickOption = { value: string; label: string };

function overlayBox(kind: Overlay["kind"]): [number, number] {
  if (kind === "environment" || kind === "templates") return [400, 480];
  if (kind === "sandbox") return [400, 280];
  return [360, 220];
}

function templatePicks(templates: Template[]): PickOption[] {
  const out: PickOption[] = [{ value: "empty", label: "empty" }];
  for (const t of templates) {
    if (t.name === "empty") continue;
    out.push({ value: t.name, label: t.shipped ? t.name : `${t.name} (saved)` });
  }
  return out;
}

function FieldSelect(props: {
  value: string;
  options: PickOption[];
  onChange: (value: string) => void;
  placeholder?: string;
}) {
  const [open, setOpen] = createSignal(false);
  const current = () =>
    props.options.find((o) => o.value === props.value)?.label ?? props.placeholder ?? "";
  onSettled(() => {
    if (!open()) return;
    const close = () => setOpen(false);
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  });
  return (
    <div class="relative mt-0.5" onMouseDown={(e) => e.stopPropagation()}>
      <button
        type="button"
        class={`${field} mt-0 flex items-center justify-between gap-2 text-left`}
        onClick={() => setOpen(!open())}
      >
        <span class="min-w-0 truncate">{current()}</span>
        <span class="shrink-0 text-[10px] leading-none">{open() ? "▴" : "▾"}</span>
      </button>
      <Show when={open()}>
        <div class="absolute top-full left-0 z-40 max-h-40 w-full overflow-auto border border-twm-line border-t-0 bg-night-raised">
          <For each={props.options} keyed={(o) => o.value}>
            {(o) => (
              <button
                type="button"
                class={`${pickItem}${o().value === props.value ? " bg-twm text-white" : ""}`}
                onClick={() => {
                  props.onChange(o().value);
                  setOpen(false);
                }}
              >
                {o().label}
              </button>
            )}
          </For>
        </div>
      </Show>
    </div>
  );
}

function overlayTitle(ov: Overlay, sandboxName: string): string {
  switch (ov.kind) {
    case "sandbox":
      return "New Sandbox";
    case "sandboxes":
      return "Sandboxes";
    case "limits":
      return `Limits — ${sandboxName}`;
    case "environment":
      return `Environment — ${sandboxName}`;
    case "templates":
      return "Templates";
    case "save-template":
      return `Save ${sandboxName} as Template`;
    case "attach":
      return "Attach Host";
    case "hosts":
      return "Hosts";
    case "destroy":
      return `Destroy ${sandboxName}`;
    case "reset":
      return `Reset ${sandboxName}`;
  }
}

function parseSettings(text: string): JsonObject | undefined {
  try {
    // SAFETY: JSON.parse is untyped; isJsonObject is the parser at this boundary.
    const parsed = JSON.parse(text || "{}") as Json | null;
    if (!isJsonObject(parsed)) return undefined;
    return parsed;
  } catch {
    return undefined;
  }
}

function applyTemplateDoc(
  name: string,
  setEnvCfg: (v: { [name: string]: EnvProgram }) => void,
): void {
  if (!name) return;
  api
    .template(name)
    .then((cfg) => {
      setEnvCfg(cfg.programs ?? {});
    })
    .catch(() => {});
}

function environmentBody(doc: EnvironmentDoc, cfg: { [name: string]: EnvProgram }): EnvironmentDoc {
  return { ...doc, programs: cfg };
}

export function OverlayDialog(props: {
  overlay: Overlay;
  sandbox?: Sandbox;
  sandboxes: Sandbox[];
  hosts?: HostRec[];
  apiFor?: (hostId?: string) => ReturnType<typeof apiOn>;
  onAttach?: (host: HostRec) => void;
  onDetach?: (id: string) => void;
  move: (x: number, y: number) => void;
  pickSandbox: (id: string) => void;
  close: () => void;
  busy?: boolean;
  refresh?: () => void;
  run: (fn: () => Promise<void>, done?: string, log?: boolean) => Promise<boolean>;
}) {
  const [name, setName] = createSignal("");
  const [cpu, setCpu] = createSignal(String(props.sandbox?.limits.cpu ?? 2));
  const [ram, setRam] = createSignal(
    String((props.sandbox?.limits.ram ?? 2147483648) / (1024 * 1024)),
  );
  const [disk, setDisk] = createSignal(
    String((props.sandbox?.limits.disk ?? 8589934592) / (1024 * 1024 * 1024)),
  );
  const [tpl, setTpl] = createSignal("empty");
  const [templates, setTemplates] = createSignal<Template[]>([]);
  const [agents, setAgents] = createSignal<AgentProgram[]>([]);
  const [envDoc, setEnvDoc] = createSignal<EnvironmentDoc>({});
  const [envCfg, setEnvCfg] = createSignal<{ [name: string]: EnvProgram }>({});
  const [attachUrl, setAttachUrl] = createSignal("");
  const [attachToken, setAttachToken] = createSignal("");
  const [hostPick, setHostPick] = createSignal(props.hosts?.[0]?.id ?? "");
  const [customize, setCustomize] = createSignal(false);
  const [tplName, setTplName] = createSignal("");
  const [optErr, setOptErr] = createSignal("");
  const start = overlayBox(props.overlay.kind);
  const [boxW, setBoxW] = createSignal(start[0]);
  const [boxH, setBoxH] = createSignal(start[1]);

  onSettled(() => {
    loadOverlay(props.overlay, props.sandbox, clientOf(props, hostPick() || props.sandbox?.host), {
      setTemplates,
      setAgents,
      setEnvDoc,
      setEnvCfg,
      setOptErr,
    });
  });

  const sbName = () => props.sandbox?.name ?? "";

  const submit = () =>
    submitOverlay(props, {
      name: name(),
      tpl: tpl(),
      cpu: cpu(),
      ram: ram(),
      disk: disk(),
      attachUrl: attachUrl(),
      attachToken: attachToken(),
      hostPick: hostPick(),
      envDoc: envDoc(),
      envCfg: envCfg(),
      customize: customize(),
      tplName: tplName(),
    });

  return (
    <Frame
      title={overlayTitle(props.overlay, sbName())}
      x={props.overlay.x}
      y={props.overlay.y}
      w={boxW()}
      h={boxH()}
      z={overlayZ}
      onMove={props.move}
      onResize={(w, h) => {
        setBoxW(w);
        setBoxH(h);
      }}
      onClose={props.close}
    >
      <form
        class="flex h-full min-h-0 flex-col bg-night-surface font-twm text-night-text"
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
        onMouseDown={(e) => e.stopPropagation()}
      >
        <div class="min-h-0 flex-1 overflow-auto px-3.5 py-3">
          <OverlayFields
            overlay={props.overlay}
            sandboxName={sbName()}
            sandboxes={props.sandboxes}
            name={name()}
            setName={setName}
            tpl={tpl()}
            setTpl={setTpl}
            templates={templates()}
            cpu={cpu()}
            setCpu={setCpu}
            ram={ram()}
            setRam={setRam}
            disk={disk()}
            setDisk={setDisk}
            attachUrl={attachUrl()}
            setAttachUrl={setAttachUrl}
            attachToken={attachToken()}
            setAttachToken={setAttachToken}
            hosts={props.hosts ?? []}
            hostPick={hostPick()}
            setHostPick={setHostPick}
            onDetach={props.onDetach}
            agents={agents()}
            envCfg={envCfg()}
            setEnvCfg={setEnvCfg}
            customize={customize()}
            setCustomize={setCustomize}
            tplName={tplName()}
            setTplName={setTplName}
            optErr={optErr()}
            pickSandbox={props.pickSandbox}
            run={props.run}
          />
        </div>
        <Show when={props.overlay.kind !== "sandboxes"}>
          <div class="flex shrink-0 justify-end gap-2 border-t border-twm-line px-3.5 py-2">
            <button type="button" class={push} onClick={props.close}>
              Cancel
            </button>
            <button type="submit" class={push} disabled={props.busy}>
              {submitLabel(props.overlay.kind, sbName())}
            </button>
          </div>
        </Show>
      </form>
    </Frame>
  );
}

function submitLabel(kind: Overlay["kind"], sandboxName: string): string {
  if (kind === "destroy") return `Destroy ${sandboxName}`;
  if (kind === "reset") return `Reset ${sandboxName}`;
  return "OK";
}

function clientOf(
  props: { apiFor?: (hostId?: string) => ReturnType<typeof apiOn>; sandbox?: Sandbox },
  hostId?: string,
) {
  if (props.apiFor) return props.apiFor(hostId ?? props.sandbox?.host);
  return api;
}

function loadOverlay(
  overlay: Overlay,
  sandbox: Sandbox | undefined,
  client: ReturnType<typeof apiOn>,
  set: {
    setTemplates: (t: Template[]) => void;
    setAgents: (a: AgentProgram[]) => void;
    setEnvDoc: (d: EnvironmentDoc) => void;
    setEnvCfg: (c: { [name: string]: EnvProgram }) => void;
    setOptErr: (v: string) => void;
  },
): void {
  if (
    overlay.kind === "sandbox" ||
    overlay.kind === "save-template" ||
    overlay.kind === "templates"
  ) {
    client
      .templates()
      .then((r) => set.setTemplates(r.templates))
      .catch(() => {});
  }
  if (
    overlay.kind === "sandbox" ||
    overlay.kind === "templates" ||
    overlay.kind === "environment"
  ) {
    client
      .agentOptions()
      .then((opts) => {
        set.setOptErr("");
        set.setAgents(opts.programs);
      })
      .catch((err) => {
        set.setAgents([]);
        set.setOptErr(err instanceof Error ? err.message : "agent options failed");
      });
  }
  if (overlay.kind === "environment" && sandbox) {
    client
      .environment(sandbox.id)
      .then((cfg) => {
        set.setEnvDoc(cfg);
        set.setEnvCfg(cfg.programs ?? {});
      })
      .catch(() => {});
  }
}

async function submitOverlay(
  props: {
    overlay: Overlay;
    sandbox?: Sandbox;
    apiFor?: (hostId?: string) => ReturnType<typeof apiOn>;
    onAttach?: (host: HostRec) => void;
    close: () => void;
    refresh?: () => void;
    run: (fn: () => Promise<void>, done?: string, log?: boolean) => Promise<boolean>;
  },
  form: {
    name: string;
    tpl: string;
    cpu: string;
    ram: string;
    disk: string;
    attachUrl: string;
    attachToken: string;
    hostPick: string;
    envDoc: EnvironmentDoc;
    envCfg: { [name: string]: EnvProgram };
    customize: boolean;
    tplName: string;
  },
): Promise<void> {
  const ov = props.overlay;
  const client = clientOf(props, ov.kind === "sandbox" ? form.hostPick : props.sandbox?.host);
  if (ov.kind === "sandboxes" || ov.kind === "hosts") {
    props.close();
    return;
  }
  if (ov.kind === "save-template" && !form.name.trim()) return;

  let ok = false;
  if (ov.kind === "attach") {
    ok = await submitAttach(props, form);
  } else if (ov.kind === "sandbox") {
    props.close();
    await props.run(
      async () => {
        const env = form.customize ? environmentBody(form.envDoc, form.envCfg) : undefined;
        const sb = await client.create(form.name || undefined, form.tpl || undefined, env);
        await client.openWindow(sb.id);
        props.refresh?.();
        await client.start(sb.id);
      },
      "",
      true,
    );
    return;
  } else if (ov.kind === "templates") {
    if (!form.tplName.trim()) return;
    ok = await props.run(() =>
      client
        .saveTemplateConfig(form.tplName.trim(), environmentBody(form.envDoc, form.envCfg))
        .then(() => undefined),
    );
  } else if (ov.kind === "save-template") {
    ok = await props.run(() => client.saveTemplate(form.name.trim(), ov.id).then(() => undefined));
  } else if (ov.kind === "limits") {
    ok = await props.run(() =>
      client
        .patchLimits(ov.id, {
          cpu: Number(form.cpu),
          ram: Number(form.ram) * 1024 * 1024,
          disk: Number(form.disk) * 1024 * 1024 * 1024,
        })
        .then(() => undefined),
    );
  } else if (ov.kind === "environment") {
    const running = props.sandbox?.state === "running";
    ok = await props.run(
      () =>
        client
          .saveEnvironment(ov.id, environmentBody(form.envDoc, form.envCfg))
          .then(() => undefined),
      running ? "" : "applies on Start",
      running,
    );
  } else if (ov.kind === "destroy") {
    ok = await props.run(() => client.destroy(ov.id));
  } else if (ov.kind === "reset") {
    ok = await props.run(() => client.reset(ov.id).then(() => undefined), "", true);
  }
  if (ok) props.close();
}

async function submitAttach(
  props: {
    onAttach?: (host: HostRec) => void;
    run: (fn: () => Promise<void>, done?: string, log?: boolean) => Promise<boolean>;
  },
  form: { attachUrl: string; attachToken: string },
): Promise<boolean> {
  if (!form.attachUrl.trim() || !form.attachToken.trim()) return false;
  return props.run(async () => {
    const host = await attachHost(form.attachUrl, form.attachToken);
    props.onAttach?.(host);
  });
}

function OverlayFields(props: {
  overlay: Overlay;
  sandboxName: string;
  sandboxes: Sandbox[];
  name: string;
  setName: (v: string) => void;
  tpl: string;
  setTpl: (v: string) => void;
  templates: Template[];
  cpu: string;
  setCpu: (v: string) => void;
  ram: string;
  setRam: (v: string) => void;
  disk: string;
  setDisk: (v: string) => void;
  attachUrl: string;
  setAttachUrl: (v: string) => void;
  attachToken: string;
  setAttachToken: (v: string) => void;
  hosts: HostRec[];
  hostPick: string;
  setHostPick: (v: string) => void;
  onDetach?: (id: string) => void;
  agents: AgentProgram[];
  envCfg: { [name: string]: EnvProgram };
  setEnvCfg: (v: { [name: string]: EnvProgram }) => void;
  customize: boolean;
  setCustomize: (v: boolean) => void;
  tplName: string;
  setTplName: (v: string) => void;
  optErr: string;
  pickSandbox: (id: string) => void;
  run: (fn: () => Promise<void>, done?: string, log?: boolean) => Promise<boolean>;
}) {
  const ov = () => props.overlay;
  return (
    <>
      <Show when={ov().kind === "sandboxes"}>
        <SandboxList sandboxes={props.sandboxes} pick={props.pickSandbox} />
      </Show>
      <Show when={ov().kind === "destroy"}>
        <div class="flex flex-col gap-2">
          <p class="text-[13px]">Destroy {props.sandboxName}?</p>
          <p class="text-[13px] font-normal leading-[18px] text-twm-muted">Workspace is gone.</p>
        </div>
      </Show>
      <Show when={ov().kind === "reset"}>
        <div class="flex flex-col gap-2">
          <p class="text-[13px]">Reset {props.sandboxName}?</p>
          <p class="text-[13px] font-normal leading-[18px] text-twm-muted">
            Puts this Sandbox back to Create. The project stays. Linux home (logins, extra files) is
            wiped. The form goes back to how it was at Create.
          </p>
        </div>
      </Show>
      <Show when={ov().kind === "sandbox"}>
        <NewSandboxFields
          name={props.name}
          setName={props.setName}
          tpl={props.tpl}
          setTpl={props.setTpl}
          templates={props.templates}
          customize={props.customize}
          setCustomize={props.setCustomize}
          agents={props.agents}
          envCfg={props.envCfg}
          setEnvCfg={props.setEnvCfg}
          optErr={props.optErr}
          hosts={props.hosts}
          hostPick={props.hostPick}
          setHostPick={props.setHostPick}
        />
      </Show>
      <Show when={ov().kind === "save-template"}>
        <label class={label}>
          name
          <input
            class={field}
            value={props.name}
            onInput={(e) => props.setName(e.currentTarget.value)}
          />
        </label>
      </Show>
      <Show when={ov().kind === "attach"}>
        <label class={label}>
          URL
          <input
            class={field}
            value={props.attachUrl}
            placeholder="http://192.168.1.5:5418"
            onInput={(e) => props.setAttachUrl(e.currentTarget.value)}
          />
        </label>
        <label class={label}>
          token
          <input
            class={field}
            value={props.attachToken}
            onInput={(e) => props.setAttachToken(e.currentTarget.value)}
          />
        </label>
      </Show>
      <Show when={ov().kind === "hosts"}>
        <HostList hosts={props.hosts} onDetach={props.onDetach} />
      </Show>
      <Show when={ov().kind === "limits"}>
        <LimitsFields
          cpu={props.cpu}
          setCpu={props.setCpu}
          ram={props.ram}
          setRam={props.setRam}
          disk={props.disk}
          setDisk={props.setDisk}
        />
      </Show>
      <Show when={ov().kind === "environment"}>
        <EnvironmentFields
          agents={props.agents}
          envCfg={props.envCfg}
          setEnvCfg={props.setEnvCfg}
          optErr={props.optErr}
        />
      </Show>
      <Show when={ov().kind === "templates"}>
        <TemplatesFields
          templates={props.templates}
          tplName={props.tplName}
          setTplName={props.setTplName}
          agents={props.agents}
          envCfg={props.envCfg}
          setEnvCfg={props.setEnvCfg}
          optErr={props.optErr}
        />
      </Show>
    </>
  );
}

function HostList(props: { hosts: HostRec[]; onDetach?: (id: string) => void }) {
  return (
    <div class="max-h-64 overflow-y-auto border border-twm-line">
      <For
        each={props.hosts}
        keyed={(h) => h.id}
        fallback={<div class="px-2 py-1 text-[12px] text-twm-muted">no Hosts Attached</div>}
      >
        {(h) => (
          <div class="flex items-center justify-between gap-2 border-t border-twm-line px-2 py-1">
            <div class="min-w-0">
              <div class="truncate font-medium">{h().label}</div>
              <div class="truncate text-[12px] text-twm-muted">{h().url}</div>
            </div>
            <button
              type="button"
              class="shrink-0 border border-twm-line bg-twm px-2 py-0.5 font-medium text-white"
              onClick={() => props.onDetach?.(h().id)}
            >
              Detach
            </button>
          </div>
        )}
      </For>
    </div>
  );
}

function SandboxList(props: { sandboxes: Sandbox[]; pick: (id: string) => void }) {
  return (
    <div class="max-h-64 overflow-y-auto border border-twm-line">
      <For
        each={props.sandboxes}
        keyed={(s) => s.id}
        fallback={<div class="px-2 py-1 text-[12px] text-twm-muted">no Sandboxes</div>}
      >
        {(s) => (
          <button
            type="button"
            class="block w-full border-0 border-t border-twm-line bg-night-surface px-2 py-1 text-left hover:bg-twm-hi hover:text-white"
            onClick={() => props.pick(s().id)}
          >
            <span class="font-medium">{s().name}</span>
            <span class="ml-2 text-[12px]">{s().state}</span>
          </button>
        )}
      </For>
    </div>
  );
}

function NewSandboxFields(props: {
  name: string;
  setName: (v: string) => void;
  tpl: string;
  setTpl: (v: string) => void;
  templates: Template[];
  customize: boolean;
  setCustomize: (v: boolean) => void;
  agents: AgentProgram[];
  envCfg: { [name: string]: EnvProgram };
  setEnvCfg: (v: { [name: string]: EnvProgram }) => void;
  optErr: string;
  hosts: HostRec[];
  hostPick: string;
  setHostPick: (v: string) => void;
}) {
  return (
    <>
      <Show when={props.hosts.length > 1}>
        <div class={label}>
          Host
          <FieldSelect
            value={props.hostPick}
            options={props.hosts.map((h) => ({ value: h.id, label: h.label }))}
            onChange={props.setHostPick}
          />
        </div>
      </Show>
      <label class={label}>
        name
        <input
          class={field}
          value={props.name}
          onInput={(e) => props.setName(e.currentTarget.value)}
        />
      </label>
      <div class={label}>
        template
        <FieldSelect
          value={props.tpl}
          options={templatePicks(props.templates)}
          onChange={(name) => {
            props.setTpl(name);
            if (props.customize) applyTemplateDoc(name, props.setEnvCfg);
          }}
        />
      </div>
      <label class="mt-2 flex items-center gap-2 font-medium">
        <input
          type="checkbox"
          checked={props.customize}
          onChange={(e) => {
            const on = e.currentTarget.checked;
            props.setCustomize(on);
            if (on) applyTemplateDoc(props.tpl, props.setEnvCfg);
          }}
        />
        Customize
      </label>
      <Show when={props.customize}>
        <EnvironmentFields
          agents={props.agents}
          envCfg={props.envCfg}
          setEnvCfg={props.setEnvCfg}
          optErr={props.optErr}
        />
      </Show>
    </>
  );
}

function LimitsFields(props: {
  cpu: string;
  setCpu: (v: string) => void;
  ram: string;
  setRam: (v: string) => void;
  disk: string;
  setDisk: (v: string) => void;
}) {
  return (
    <>
      <label class={label}>
        cpu
        <input
          class={field}
          value={props.cpu}
          onInput={(e) => props.setCpu(e.currentTarget.value)}
        />
      </label>
      <label class={label}>
        ram (MiB)
        <input
          class={field}
          value={props.ram}
          onInput={(e) => props.setRam(e.currentTarget.value)}
        />
      </label>
      <label class={label}>
        disk (GiB)
        <input
          class={field}
          value={props.disk}
          onInput={(e) => props.setDisk(e.currentTarget.value)}
        />
      </label>
    </>
  );
}

function getField(obj: JsonObject, path: string): Json | undefined {
  const parts = path.split(".");
  let cur: Json | undefined = obj;
  for (const part of parts) {
    if (!isJsonObject(cur ?? null)) return undefined;
    cur = cur[part];
  }
  return cur;
}

function setField(obj: JsonObject, path: string, value: Json): JsonObject {
  const parts = path.split(".");
  const next: JsonObject = { ...obj };
  const head = parts[0];
  if (parts.length === 1) {
    next[head] = value;
    return next;
  }
  const child = isJsonObject(next[head] ?? null) ? next[head] : {};
  next[head] = setField(child, parts.slice(1).join("."), value);
  return next;
}

function jsonText(value: Json | undefined): string {
  return JSON.stringify(value ?? {}, null, 2);
}

function selectedPackages(value: Json | undefined): string[] {
  if (!Array.isArray(value)) return [];
  const out: string[] = [];
  for (const item of value) {
    if (isJsonObject(item) || item === null || Array.isArray(item)) continue;
    out.push(String(item));
  }
  return out;
}

function nixAttr(name: string): boolean {
  return /^[A-Za-z_][A-Za-z0-9_'-]*$/.test(name);
}

function OptionField(props: {
  opt: AgentOption;
  cur: JsonObject;
  onSet: (next: JsonObject) => void;
}) {
  const [draft, setDraft] = createSignal("");
  const opt = () => props.opt;
  const value = () => getField(props.cur, opt().name);
  if (opt().type === "boolean") {
    return (
      <label class="mt-1.5 flex items-center gap-2 font-medium">
        <input
          type="checkbox"
          checked={Boolean(value())}
          onChange={(e) => props.onSet(setField(props.cur, opt().name, e.currentTarget.checked))}
        />
        {opt().name}
      </label>
    );
  }
  if (opt().type === "packageNames") {
    const selected = () => selectedPackages(value());
    const draftName = () => draft().trim();
    const invalid = () => draftName() !== "" && !nixAttr(draftName());
    return (
      <div class="mt-1.5">
        <div class={label}>{opt().name}</div>
        <For each={selected()} keyed={(n) => n}>
          {(pkg) => {
            const name = pkg();
            return (
              <div class="mt-0.5 flex items-center gap-1 text-[12px]">
                <span class="font-mono">{name}</span>
                <button
                  type="button"
                  class="border border-twm-line bg-twm px-2 py-0.5 font-medium text-white"
                  onClick={() => {
                    props.onSet(
                      setField(
                        props.cur,
                        opt().name,
                        selected().filter((n) => n !== name),
                      ),
                    );
                  }}
                >
                  remove
                </button>
              </div>
            );
          }}
        </For>
        <div class="mt-1 flex gap-1">
          <input
            class={field}
            value={draft()}
            placeholder="nixpkgs name"
            onInput={(e) => setDraft(e.currentTarget.value)}
          />
          <button
            type="button"
            class="border border-twm-line bg-twm px-2 py-0.5 font-medium text-white"
            onClick={() => {
              const name = draftName();
              if (!name || invalid()) return;
              if (selected().includes(name)) {
                setDraft("");
                return;
              }
              props.onSet(setField(props.cur, opt().name, [...selected(), name]));
              setDraft("");
            }}
          >
            add
          </button>
        </div>
        <Show when={invalid()}>
          <p class="text-[12px] text-red-400">invalid package name</p>
        </Show>
      </div>
    );
  }
  if (opt().type === "stringMap") {
    const obj = (): JsonObject => (isJsonObject(value() ?? null) ? value() : {});
    const keys = () => Object.keys(obj());
    return (
      <div class="mt-1.5">
        <div class={label}>{opt().name}</div>
        <For each={keys()}>
          {(k) => (
            <label class={label}>
              {k()}
              <textarea
                class={`${field} h-16`}
                value={String(obj()[k()] ?? "")}
                onChange={(e) => {
                  const next: JsonObject = { ...obj(), [k()]: e.currentTarget.value };
                  props.onSet(setField(props.cur, opt().name, next));
                }}
              />
            </label>
          )}
        </For>
        <div class="mt-1 flex gap-1">
          <input
            class={field}
            value={draft()}
            placeholder="name"
            onInput={(e) => setDraft(e.currentTarget.value)}
          />
          <button
            type="button"
            class="border border-twm-line bg-twm px-2 py-0.5 font-medium text-white"
            onClick={() => {
              const k = draft().trim();
              if (!k) return;
              const next: JsonObject = { ...obj(), [k]: "" };
              props.onSet(setField(props.cur, opt().name, next));
              setDraft("");
            }}
          >
            add
          </button>
        </div>
      </div>
    );
  }
  if (opt().type === "string" || opt().type === "number") {
    return (
      <label class={label}>
        {opt().name}
        <input
          class={field}
          value={value() === undefined || value() === null ? "" : String(value())}
          onInput={(e) => {
            const raw = e.currentTarget.value;
            const next = opt().type === "number" ? (raw === "" ? 0 : Number(raw)) : raw;
            props.onSet(setField(props.cur, opt().name, next));
          }}
        />
      </label>
    );
  }
  return (
    <label class={label}>
      {opt().name}
      <textarea
        class={`${field} h-24`}
        value={jsonText(value())}
        onChange={(e) => {
          const parsed = parseSettings(e.currentTarget.value);
          if (!parsed) return;
          props.onSet(setField(props.cur, opt().name, parsed));
        }}
      />
    </label>
  );
}

function EnvironmentFields(props: {
  agents: AgentProgram[];
  envCfg: { [name: string]: EnvProgram };
  setEnvCfg: (v: { [name: string]: EnvProgram }) => void;
  optErr?: string;
}) {
  return (
    <>
      <Show when={props.optErr}>
        <p class="text-[12px] text-red-400">{props.optErr}</p>
      </Show>
      <p class="text-[12px]">
        devenv is always in the Environment. Keys are not stored in the recipe.
      </p>
      <For
        each={props.agents}
        keyed={(p) => p.name}
        fallback={<div class="text-[12px]">no Agents</div>}
      >
        {(p) => {
          const name = p().name;
          const cur = () => props.envCfg[name] ?? {};
          const enabled = () => Boolean(cur().enable);
          return (
            <div class="mt-2 border-t border-twm-line pt-2">
              <label class="flex items-center gap-2 font-medium">
                <input
                  type="checkbox"
                  checked={enabled()}
                  onChange={(e) => {
                    const on = e.currentTarget.checked;
                    props.setEnvCfg({
                      ...props.envCfg,
                      [name]: { ...cur(), enable: on },
                    });
                  }}
                />
                {name}
              </label>
              <div class="text-[11px] text-twm-muted">{p().description}</div>
              <For each={p().options.filter((o) => o.name !== "enable")} keyed={(o) => o.name}>
                {(o) => (
                  <OptionField
                    opt={o()}
                    cur={cur()}
                    onSet={(next) => props.setEnvCfg({ ...props.envCfg, [name]: next })}
                  />
                )}
              </For>
            </div>
          );
        }}
      </For>
    </>
  );
}

function TemplatesFields(props: {
  templates: Template[];
  tplName: string;
  setTplName: (v: string) => void;
  agents: AgentProgram[];
  envCfg: { [name: string]: EnvProgram };
  setEnvCfg: (v: { [name: string]: EnvProgram }) => void;
  optErr: string;
}) {
  const selected = () => props.templates.find((t) => t.name === props.tplName);
  const userTemplates = () => props.templates.filter((t) => !t.shipped);
  return (
    <>
      <p class="text-[12px]">
        Saved Templates only. empty cannot be overwritten. Existing Sandboxes are not changed.
      </p>
      <div class={label}>
        template
        <FieldSelect
          value={props.tplName}
          placeholder="pick a saved Template"
          options={[
            { value: "", label: "pick a saved Template" },
            ...userTemplates().map((t) => ({ value: t.name, label: t.name })),
          ]}
          onChange={(name) => {
            props.setTplName(name);
            applyTemplateDoc(name, props.setEnvCfg);
          }}
        />
      </div>
      <Show when={props.tplName && selected()}>
        <EnvironmentFields
          agents={props.agents}
          envCfg={props.envCfg}
          setEnvCfg={props.setEnvCfg}
          optErr={props.optErr}
        />
      </Show>
    </>
  );
}
