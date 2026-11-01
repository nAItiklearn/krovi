<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import Icon from '../components/Icon.svelte';
  import { copyText, now, pushToast, transferRequest } from '../lib/stores';
  import { formatBytes, relativeTime } from '../lib/time';
  import { arrive, depart } from '../lib/motion';
  import { isTauri } from '../lib/bridge';
  import type { IconName } from '../lib/icons';
  import * as tx from '../lib/transfer';
  import { config, progress, received, shares } from '../lib/transfer';

  // Quick Transfer: drop a file -> a QR appears -> the phone scans it and downloads over the
  // same Wi-Fi. "Receive from phone" opens an upload page the other way. LAN only, no cloud.
  // The Rust side (krovi-transfer) runs a server only while a share is open; this view is the
  // remote control and the live status.

  let tab = $state<'send' | 'receive'>('send');
  let over = $state(false);
  let busy = $state(false);
  let notice = $state<string | null>(null);
  let copied = $state(false);
  let report = $state<tx.FirewallReport | null>(null);
  let checking = $state(false);
  let allowing = $state(false);
  let fileInput: HTMLInputElement | undefined = $state();

  const sendShare = $derived($shares.find((s) => s.mode === 'send') ?? null);
  const recvShare = $derived($shares.find((s) => s.mode === 'receive') ?? null);
  const active = $derived(tab === 'send' ? sendShare : recvShare);
  const hint = $derived(active ? tx.needsHint(active, $now) : false);
  const lines = $derived(tx.diagnose(report));
  const liveUploads = $derived(
    recvShare ? Object.values($progress).filter((p) => p.id === recvShare.id) : [],
  );

  const TTL_CHOICES = [
    { v: 600, label: '10 minutes' },
    { v: 1800, label: '30 minutes' },
    { v: 3600, label: '1 hour' },
  ];
  const CAP_CHOICES = [
    { v: 500 * 1024 ** 2, label: '500 MB' },
    { v: 2 * 1024 ** 3, label: '2 GB' },
    { v: 10 * 1024 ** 3, label: '10 GB' },
    { v: 50 * 1024 ** 3, label: '50 GB' },
  ];

  const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));

  function iconFor(name: string): IconName {
    const ext = name.split('.').pop()?.toLowerCase() ?? '';
    if (['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'heic', 'bmp'].includes(ext)) return 'image';
    if (['pdf', 'doc', 'docx', 'txt', 'md', 'rtf', 'xls', 'xlsx', 'csv'].includes(ext)) return 'fileText';
    return 'file';
  }

  async function startSend(inputs: tx.ShareInput[]) {
    if (!inputs.length || busy) return;
    busy = true;
    notice = null;
    report = null;
    tab = 'send';
    try {
      if (sendShare) await tx.stopShare(sendShare.id);
      await tx.shareFiles(inputs);
    } catch (e) {
      notice = msg(e);
    } finally {
      busy = false;
    }
  }

  async function startReceive() {
    if (busy) return;
    busy = true;
    notice = null;
    report = null;
    try {
      await tx.receiveFromPhone();
    } catch (e) {
      notice = msg(e);
    } finally {
      busy = false;
    }
  }

  async function stop(id: string) {
    try {
      await tx.stopShare(id);
    } catch (e) {
      notice = msg(e);
    }
    report = null;
  }

  async function choose() {
    if (!isTauri()) {
      fileInput?.click();
      return;
    }
    try {
      const paths = await tx.pickFiles();
      if (paths.length) await startSend(paths);
    } catch (e) {
      notice = msg(e);
    }
  }

  function onPicked(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const files = Array.from(input.files ?? []).map((f) => ({ name: f.name, size: f.size }));
    input.value = '';
    void startSend(files);
  }

  // Browser demo (and any non-Tauri host): normal DOM drops. In Tauri the webview reports OS
  // drops as paths through onFileDrop instead (see onMount).
  const hasFiles = (e: DragEvent) => Array.from(e.dataTransfer?.types ?? []).includes('Files');
  function onDragOver(e: DragEvent) {
    if (!hasFiles(e)) return;
    e.preventDefault();
    over = true;
  }
  function onDrop(e: DragEvent) {
    over = false;
    if (!hasFiles(e)) return;
    e.preventDefault();
    const files = Array.from(e.dataTransfer?.files ?? []).map((f) => ({ name: f.name, size: f.size }));
    void startSend(files);
  }

  async function copyLink(url: string) {
    if (await copyText(url)) {
      copied = true;
      setTimeout(() => (copied = false), 1400);
    } else {
      pushToast('Could not copy the link');
    }
  }

  async function checkNetwork() {
    checking = true;
    try {
      report = await tx.firewallStatus();
    } catch (e) {
      notice = msg(e);
    } finally {
      checking = false;
    }
  }

  async function allowFirewall() {
    allowing = true;
    try {
      report = await tx.firewallAllow();
      pushToast(report.ruleInstalled ? 'Firewall rule added for Private networks' : 'Firewall rule not added');
    } catch (e) {
      notice = msg(e);
    } finally {
      allowing = false;
    }
  }

  async function openFolder() {
    try {
      await tx.openReceiveFolder();
    } catch (e) {
      notice = msg(e);
    }
  }

  async function reveal(path: string | null) {
    if (!path) return openFolder();
    try {
      await tx.revealReceived(path);
    } catch (e) {
      notice = msg(e);
    }
  }

  const remaining = (s: tx.Share) => tx.msLeft(s, $now);
  const lifeFraction = (s: tx.Share) => {
    const total = Math.max(1, s.expiresAt - (s.createdAt || s.expiresAt - $config.ttlSecs * 1000));
    return Math.max(0, Math.min(1, remaining(s) / total));
  };

  // "Send this to my phone" from elsewhere in the app (voice command, shelf, palette).
  $effect(() => {
    const r = $transferRequest;
    if (!r) return;
    transferRequest.set(null);
    untrack(() => void startSend(r.paths));
  });

  onMount(() => {
    tx.setToaster(pushToast);
    void tx.startTransfer();
    let off: (() => void) | undefined;
    let dead = false;
    void tx
      .onFileDrop(
        (paths) => void startSend(paths),
        (o) => (over = o),
      )
      .then((u) => (dead ? u() : (off = u)));
    return () => {
      dead = true;
      off?.();
    };
  });
</script>

{#snippet phone()}
  <svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <rect x="6.5" y="2.5" width="11" height="19" rx="2.6" />
    <path d="M10.5 18.5h3" />
  </svg>
{/snippet}

{#snippet qrCard(s: tx.Share)}
  <div class="qrcard card">
    <div class="qr">{@html s.qrSvg}</div>
    {#if s.demo}<span class="tag accent demo">Demo: this QR goes nowhere</span>{/if}
    <div class="url">
      <code>{s.url}</code>
      <button class="btn icon sm" aria-label={copied ? 'Link copied' : 'Copy link'} onclick={() => copyLink(s.url)}>
        <Icon name={copied ? 'check' : 'copy'} size={14} />
      </button>
    </div>
    <div class="expiry">
      <span class="faint"><Icon name="clock" size={13} /> Expires in</span>
      <strong role="timer" aria-live="off">{tx.countdown(remaining(s))}</strong>
    </div>
    <div class="life" aria-hidden="true"><i style:width="{lifeFraction(s) * 100}%"></i></div>
    {#if s.pin}
      <div class="pin" title="Your phone asks for this PIN before it can see anything">
        <Icon name="lock" size={14} /><span class="faint">PIN</span><b>{s.pin}</b>
      </div>
    {/if}
    <button class="btn danger stop" onclick={() => stop(s.id)}><Icon name="close" size={14} /> Stop sharing</button>
  </div>
{/snippet}

{#snippet status(s: tx.Share)}
  <div class="status" class:ok={s.reached} role="status">
    <span class="dot" aria-hidden="true"></span>
    {#if s.reached}
      Phone connected{s.mode === 'send' ? ` · ${s.downloads} ${s.downloads === 1 ? 'download' : 'downloads'}` : ` · ${s.uploads} received`}
    {:else}
      Waiting for your phone…
    {/if}
  </div>
{/snippet}

{#snippet hintCard()}
  <div class="hint card card-pad" role="region" aria-label="Phone cannot connect">
    <div class="hint-h"><Icon name="alert" size={16} /> <h2>Phone can’t connect yet?</h2></div>
    <ul>
      <li>Make sure the phone is on the same Wi-Fi as this PC (not mobile data, not a guest network).</li>
      <li>Allow Krovi on <b>Private networks</b> in the Windows firewall prompt.</li>
    </ul>
    {#if lines.length}
      <ul class="diag">
        {#each lines as l (l)}<li>{l}</li>{/each}
      </ul>
    {/if}
    <div class="hint-actions">
      <button class="btn sm" onclick={checkNetwork} disabled={checking}>
        <Icon name={checking ? 'loader' : 'refresh'} size={13} /> {checking ? 'Checking…' : 'Check network'}
      </button>
      <button class="btn sm" onclick={allowFirewall} disabled={allowing} title="Asks Windows for admin permission once">
        <Icon name="shield" size={13} /> {allowing ? 'Waiting for Windows…' : 'Allow Krovi in the firewall'}
      </button>
    </div>
    <p class="fine faint">
      The firewall button asks Windows for permission (a UAC prompt) and adds one rule: this app only, Private networks
      only, local network only. Nothing else changes, and nothing happens unless you click it.
    </p>
  </div>
{/snippet}

<div class="view" ondragover={onDragOver} ondragleave={() => (over = false)} ondrop={onDrop} role="presentation">
  <header class="view-head">
    <p class="lede">Over your Wi-Fi only. No cloud, no account, no cable.</p>
    <div class="tabs" role="tablist" aria-label="Direction">
      <button class="chip" class:on={tab === 'send'} role="tab" aria-selected={tab === 'send'} onclick={() => (tab = 'send')}>
        <Icon name="upload" size={13} /> Send to phone{#if sendShare}<span class="live" aria-label="active"></span>{/if}
      </button>
      <button class="chip" class:on={tab === 'receive'} role="tab" aria-selected={tab === 'receive'} onclick={() => (tab = 'receive')}>
        <Icon name="download" size={13} /> Receive from phone{#if recvShare}<span class="live" aria-label="active"></span>{/if}
      </button>
    </div>
  </header>

  {#if notice}
    <div class="notice" role="alert">
      <Icon name="alert" size={15} /> <span>{notice}</span>
      <button class="btn icon sm ghost" aria-label="Dismiss" onclick={() => (notice = null)}><Icon name="close" size={13} /></button>
    </div>
  {/if}

  <div class="body scroll">
    {#if !active}
      <!-- ---------------- nothing shared yet ---------------- -->
      <div class="idle" in:arrive>
        {#if tab === 'send'}
          <section class="drop" class:over aria-label="Drop files to send">
            <div class="glyph">{@render phone()}</div>
            <h2>Drop files here</h2>
            <p>A QR code appears instantly. Scan it with your phone’s camera to download the files over your Wi-Fi.</p>
            <button class="btn primary" onclick={choose} disabled={busy}><Icon name="plus" size={14} /> Choose files</button>
            <input bind:this={fileInput} type="file" multiple hidden onchange={onPicked} />
          </section>
        {:else}
          <section class="drop" aria-label="Receive files from your phone">
            <div class="glyph">{@render phone()}</div>
            <h2>Receive from your phone</h2>
            <p>
              Opens an upload page for your phone. Photos, videos and files land in
              <b>Downloads\Krovi</b> on this PC.
            </p>
            <button class="btn primary" onclick={startReceive} disabled={busy}><Icon name="download" size={14} /> Receive from phone</button>
          </section>
        {/if}

        <aside class="opts card card-pad" aria-label="Sharing options">
          <label class="opt">
            <span><b>Require a PIN</b><small class="faint">A 4-digit code shown here, asked on the phone</small></span>
            <button
              class="toggle"
              role="switch"
              aria-checked={$config.requirePin}
              aria-label="Require a PIN"
              onclick={() => tx.updateConfig({ requirePin: !$config.requirePin })}></button>
          </label>
          <label class="opt">
            <span><b>Link expires after</b><small class="faint">It also closes when you stop it or quit Krovi</small></span>
            <select class="input sel" value={$config.ttlSecs} onchange={(e) => tx.updateConfig({ ttlSecs: Number(e.currentTarget.value) })}>
              {#each TTL_CHOICES as c (c.v)}<option value={c.v}>{c.label}</option>{/each}
            </select>
          </label>
          {#if tab === 'receive'}
            <label class="opt">
              <span><b>Upload limit</b><small class="faint">Per file, and in total for one link</small></span>
              <select class="input sel" value={$config.maxUploadBytes} onchange={(e) => tx.updateConfig({ maxUploadBytes: Number(e.currentTarget.value) })}>
                {#each CAP_CHOICES as c (c.v)}<option value={c.v}>{c.label}</option>{/each}
              </select>
            </label>
          {/if}
          <p class="fine faint">
            <Icon name="shield" size={12} /> Local network only. Krovi opens a private door while a link is open and
            closes it afterwards.
          </p>
        </aside>
      </div>
    {:else}
      <!-- ---------------- a share is live ---------------- -->
      <div class="live-grid" in:arrive>
        {@render qrCard(active)}

        <section class="files" aria-label={active.mode === 'send' ? 'Files being shared' : 'Files from your phone'}>
          <div class="files-h">
            <div>
              <h2>
                {#if active.mode === 'send'}
                  {active.files.length} {active.files.length === 1 ? 'file' : 'files'} ready
                {:else if active.files.length || liveUploads.length}
                  Receiving from your phone
                {:else}
                  Waiting for uploads
                {/if}
              </h2>
              {@render status(active)}
            </div>
            {#if active.mode === 'receive'}
              <button class="btn sm" onclick={openFolder}><Icon name="folder" size={13} /> Open folder</button>
            {/if}
          </div>

          {#if hint}
            <div in:arrive>{@render hintCard()}</div>
          {/if}

          {#if active.mode === 'send'}
            <ul class="rows scroll">
              {#each active.files as f (f.name)}
                {@const p = $progress[tx.progressKey(active.id, f.name)]}
                <li class="frow" in:arrive out:depart>
                  <span class="fi"><Icon name={iconFor(f.name)} size={15} /></span>
                  <span class="fn ellipsis" title={f.name}>{f.name}</span>
                  <span class="fs faint">{formatBytes(f.size)}</span>
                  {#if p}
                    <span class="pct" aria-label="{tx.percent(p.bytes, p.total)} percent">{tx.percent(p.bytes, p.total)}%</span>
                    <span class="bar" aria-hidden="true"><i style:width="{tx.percent(p.bytes, p.total)}%"></i></span>
                  {:else if active.sent?.includes(f.name)}
                    <span class="ok" role="img" aria-label="Sent to your phone"><Icon name="circleCheck" size={16} /></span>
                  {/if}
                </li>
              {/each}
            </ul>
          {:else}
            <div class="cap faint">Limit {formatBytes(active.maxUploadBytes)} per link · {formatBytes(active.receivedBytes)} used</div>
            <ul class="rows scroll">
              {#each liveUploads as p (p.file)}
                <li class="frow" in:arrive>
                  <span class="fi"><Icon name="download" size={15} /></span>
                  <span class="fn ellipsis" title={p.file}>{p.file}</span>
                  <span class="fs faint">{p.total > 0 ? formatBytes(p.total) : formatBytes(p.bytes)}</span>
                  <span class="pct">{p.total > 0 ? `${tx.percent(p.bytes, p.total)}%` : '…'}</span>
                  <span class="bar" aria-hidden="true"><i style:width="{p.total > 0 ? tx.percent(p.bytes, p.total) : 40}%"></i></span>
                </li>
              {/each}
              {#each active.files as f (f.name)}
                <li class="frow done" in:arrive>
                  <span class="fi"><Icon name={iconFor(f.name)} size={15} /></span>
                  <span class="fn ellipsis" title={f.name}>{f.name}</span>
                  <span class="fs faint">{formatBytes(f.size)}</span>
                  <span class="ok" aria-label="received"><Icon name="circleCheck" size={16} /></span>
                </li>
              {/each}
              {#if !liveUploads.length && !active.files.length}
                <li class="empty faint">Open the link on your phone and choose files. They appear here as they arrive.</li>
              {/if}
            </ul>
          {/if}
        </section>
      </div>
    {/if}

    {#if tab === 'receive'}
      <section class="history" aria-label="Received files">
        <div class="history-h">
          <h2>Received</h2>
          <span class="faint">{$received.length ? `${$received.length} recent` : ''}</span>
          <button class="btn sm ghost" onclick={openFolder}><Icon name="folder" size={13} /> Open folder</button>
        </div>
        {#if $received.length}
          <ul class="rows">
            {#each $received.slice(0, 8) as r (r.key)}
              <li class="frow" in:arrive>
                <span class="fi"><Icon name={iconFor(r.name)} size={15} /></span>
                <span class="fn ellipsis" title={r.savedPath ?? r.name}>{r.name}</span>
                <span class="fs faint">{formatBytes(r.size)}</span>
                <span class="fw faint">{relativeTime(r.at, $now)}</span>
                <button class="btn icon sm ghost" aria-label={`Show ${r.name} in folder`} onclick={() => reveal(r.savedPath)}>
                  <Icon name="externalLink" size={13} />
                </button>
              </li>
            {/each}
          </ul>
        {:else}
          <p class="empty faint">Files you receive show up here, and in your Downloads\Krovi folder.</p>
        {/if}
      </section>
    {/if}
  </div>
</div>

<style>
  .view-head {
    align-items: center;
  }
  .lede {
    margin: 0;
    max-width: 52ch;
    color: var(--text-2);
    font-size: var(--t-sm);
    line-height: 1.5;
  }
  .tabs {
    display: flex;
    gap: 6px;
    flex: none;
  }
  .tabs .chip {
    height: 32px;
    padding: 0 13px;
  }
  .live {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }
  .chip.on .live {
    background: var(--accent);
  }

  .body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 18px;
    padding-right: 4px;
    margin-right: -4px;
  }
  .notice {
    display: flex;
    align-items: center;
    gap: 9px;
    margin-bottom: 12px;
    padding: 8px 8px 8px 12px;
    border-radius: var(--r-2);
    border: 1px solid color-mix(in srgb, var(--danger) 40%, transparent);
    background: color-mix(in srgb, var(--danger) 10%, transparent);
    color: var(--danger);
    font-size: var(--t-sm);
    font-weight: 560;
    flex: none;
  }
  .notice span {
    flex: 1;
  }

  /* ---- idle: drop zone + options ---- */
  .idle {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 280px;
    gap: 18px;
    align-items: stretch;
    flex: 1 1 330px;
  }
  .drop {
    display: grid;
    place-items: center;
    align-content: center;
    gap: 8px;
    text-align: center;
    padding: 28px;
    border: 1.5px dashed var(--line-strong);
    border-radius: var(--r-4);
    color: var(--text-2);
    transition:
      border-color var(--dur-2),
      background var(--dur-2),
      transform 0.45s var(--spring);
  }
  .drop.over {
    border-color: var(--accent);
    background: var(--accent-soft);
    transform: scale(1.01);
  }
  .drop .glyph {
    display: grid;
    place-items: center;
    width: 54px;
    height: 54px;
    margin-bottom: 6px;
    border-radius: var(--r-3);
    background: var(--surface-1);
    border: 1px solid var(--line);
    color: var(--text);
    box-shadow: inset 0 1px 0 var(--card-hi);
  }
  .drop h2 {
    font-size: var(--t-xl);
    font-weight: 650;
    letter-spacing: -0.02em;
    color: var(--text);
  }
  .drop p {
    max-width: 38ch;
    font-size: var(--t-sm);
    line-height: 1.55;
    margin-bottom: 8px;
  }
  .opts {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  .opt {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    font-size: var(--t-sm);
  }
  .opt span {
    display: grid;
    gap: 2px;
    min-width: 0;
  }
  .opt small {
    font-size: var(--t-xs);
    line-height: 1.35;
  }
  .sel {
    width: 118px;
    flex: none;
  }
  .fine {
    font-size: var(--t-xs);
    line-height: 1.5;
    margin-top: auto;
  }

  /* ---- live share ---- */
  .live-grid {
    display: grid;
    grid-template-columns: 292px minmax(0, 1fr);
    gap: 18px;
    align-items: start;
  }
  .qrcard {
    display: flex;
    flex-direction: column;
    gap: 9px;
    padding: 14px;
  }
  .qr {
    background: var(--qr-plate);
    border-radius: var(--r-3);
    padding: 8px;
    line-height: 0;
    box-shadow: var(--shadow-sm);
  }
  .qr :global(svg) {
    display: block;
    width: 100%;
    height: auto;
  }
  .demo {
    align-self: center;
  }
  .url {
    display: flex;
    align-items: flex-start;
    gap: 6px;
  }
  .url code {
    flex: 1;
    min-width: 0;
    /* show the whole address, wrapped: it is what you type if the camera is not an option */
    white-space: normal;
    overflow-wrap: anywhere;
    line-height: 1.45;
    user-select: all;
    padding: 6px 9px;
    border-radius: var(--r-1);
    background: var(--surface-1);
    border: 1px solid var(--line);
    font-family: var(--font-mono);
    font-size: var(--t-xs);
    color: var(--text-2);
  }
  .expiry {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    font-size: var(--t-sm);
  }
  .expiry span {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .expiry strong {
    font-size: var(--t-lg);
    font-variant-numeric: tabular-nums;
    letter-spacing: -0.01em;
  }
  .life {
    height: 3px;
    border-radius: 99px;
    background: var(--surface-2);
    overflow: hidden;
    margin-top: -4px;
  }
  .life i {
    display: block;
    height: 100%;
    background: var(--text-2);
    border-radius: 99px;
    transition: width 1s linear;
  }
  .pin {
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 8px 12px;
    border-radius: var(--r-2);
    background: var(--surface-2);
    font-size: var(--t-sm);
  }
  .pin b {
    margin-left: auto;
    font-size: var(--t-xl);
    letter-spacing: 0.28em;
    font-variant-numeric: tabular-nums;
  }
  .stop {
    width: 100%;
  }

  .files {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .files-h {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
  }
  .files-h h2 {
    font-family: var(--font-display);
    font-size: var(--t-xl);
    font-weight: 650;
    letter-spacing: -0.02em;
  }
  .status {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    margin-top: 4px;
    font-size: var(--t-sm);
    color: var(--text-2);
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--text-3);
    animation: wait 1.4s var(--ease-in-out) infinite;
  }
  .status.ok {
    color: var(--text);
  }
  .status.ok .dot {
    background: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
    animation: none;
  }
  @keyframes wait {
    50% {
      opacity: 0.25;
    }
  }
  .cap {
    font-size: var(--t-xs);
  }
  .rows {
    display: flex;
    flex-direction: column;
    gap: 6px;
    max-height: 312px;
    overflow-y: auto;
  }
  .frow {
    position: relative;
    display: flex;
    align-items: center;
    gap: 11px;
    min-height: 46px;
    padding: 0 10px 0 8px;
    border-radius: var(--r-3);
    border: 1px solid var(--line);
    background: var(--surface-1);
    overflow: hidden;
  }
  .fi {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border-radius: var(--r-2);
    background: var(--surface-2);
    color: var(--text-2);
    flex: none;
  }
  .fn {
    flex: 1;
    min-width: 0;
    font-size: var(--t-sm);
    font-weight: 560;
  }
  .fs,
  .fw {
    font-size: var(--t-xs);
    white-space: nowrap;
  }
  .fs {
    min-width: 56px;
    text-align: right;
  }
  .pct {
    min-width: 36px;
    text-align: right;
    font-size: var(--t-xs);
    font-weight: 650;
    font-variant-numeric: tabular-nums;
  }
  .bar {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: 3px;
    background: var(--surface-2);
  }
  .bar i {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width 0.15s linear;
  }
  .ok {
    display: flex;
    color: var(--text);
  }
  .empty {
    padding: 14px 4px;
    font-size: var(--t-sm);
    list-style: none;
  }

  .hint {
    display: grid;
    gap: 8px;
    padding: 13px 15px;
    border-color: color-mix(in srgb, var(--accent) 40%, var(--line));
  }
  .hint-h {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--accent);
  }
  .hint-h h2 {
    font-size: var(--t-md);
    font-weight: 650;
    color: var(--text);
  }
  .hint ul {
    margin: 0;
    padding-left: 18px;
    display: grid;
    gap: 3px;
    font-size: var(--t-xs);
    color: var(--text-2);
    line-height: 1.5;
    list-style: disc;
  }
  .diag {
    padding-top: 6px;
    border-top: 1px solid var(--line);
  }
  .hint-actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }

  /* ---- received history ---- */
  .history {
    display: grid;
    gap: 8px;
  }
  .history-h {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .history-h h2 {
    font-size: var(--t-md);
    font-weight: 650;
  }
  .history-h .btn {
    margin-left: auto;
  }
  .history .rows {
    max-height: none;
  }

  @media (max-width: 760px) {
    .idle,
    .live-grid {
      grid-template-columns: 1fr;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .dot {
      animation: none;
    }
    .life i,
    .bar i {
      transition: none;
    }
  }
</style>
