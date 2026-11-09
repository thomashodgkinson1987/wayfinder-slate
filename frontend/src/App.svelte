<script lang="ts">
  import { onMount } from 'svelte';
  import {
    getConfig,
    getMe,
    getSlot,
    getSlots,
    logout,
    type AppConfig,
    type SlotDetail,
    type SlotSummary,
    type User,
  } from './lib/api';
  import Auth from './pages/Auth.svelte';
  import Dashboard from './pages/Dashboard.svelte';
  import MapView from './pages/MapView.svelte';
  import PlayData from './pages/PlayData.svelte';
  import Setup from './pages/Setup.svelte';

  type PageId = 'dashboard' | 'map' | 'play-data' | 'setup';

  const NAV: { id: PageId; label: string; icon: string }[] = [
    { id: 'dashboard', label: 'Dashboard', icon: '◈' },
    { id: 'map', label: 'Hyrule Map', icon: '⌖' },
    { id: 'play-data', label: 'Play Data', icon: '▤' },
    { id: 'setup', label: 'Setup', icon: '⚙' },
  ];

  const PAGE_META: Record<PageId, { title: string; subtitle: string }> = {
    dashboard: {
      title: 'Dashboard',
      subtitle: 'A quiet overview of your journey through Hyrule.',
    },
    map: {
      title: 'Hyrule Map',
      subtitle: 'Shrines, towers, Divine Beasts, memories, and every unfinished path.',
    },
    'play-data': {
      title: 'Play Data',
      subtitle: 'Progress, records, and the story the save file tells.',
    },
    setup: {
      title: 'Setup & About',
      subtitle: 'Deployment mode, save discovery, and data sources.',
    },
  };

  const SLOT_STORAGE_KEY = 'wayfinder-slate:selected-slot';

  let page = $state<PageId>('dashboard');
  let config = $state<AppConfig | null>(null);
  let user = $state<User | null>(null);
  let authChecked = $state(false);
  let error = $state<string | null>(null);
  let retryTimer: ReturnType<typeof setInterval> | null = null;
  let slots = $state<SlotSummary[]>([]);
  let selectedSlot = $state<number | null>(null);
  let summary = $state<SlotDetail | null>(null);
  let saveError = $state<string | null>(null);
  let summaryLoading = $state(false);
  let summaryRequest = 0;

  const meta = $derived(PAGE_META[page]);
  const modeLabel = $derived(
    config === null
      ? 'Connecting'
      : config.mode === 'hub'
        ? 'Public hub'
        : config.mode === 'lan'
          ? 'LAN'
          : 'Local',
  );
  const showSlotPicker = $derived(
    config !== null && config.mode !== 'hub' && slots.length > 0,
  );
  const showAuthGate = $derived(
    config !== null && authChecked && config.auth_required && user === null,
  );

  function readStoredSlot(): number | null {
    try {
      const raw = window.localStorage.getItem(SLOT_STORAGE_KEY);
      if (raw === null) return null;
      const value = Number(raw);
      return Number.isInteger(value) ? value : null;
    } catch {
      return null;
    }
  }

  function storeSlot(slot: number) {
    try {
      window.localStorage.setItem(SLOT_STORAGE_KEY, String(slot));
    } catch {
      // Storage can be unavailable in private browsing; selection still works.
    }
  }

  async function loadConfig() {
    try {
      config = await getConfig();
      error = null;
      if (retryTimer !== null) {
        clearInterval(retryTimer);
        retryTimer = null;
      }
      await loadUser();
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    }
  }

  async function loadUser() {
    try {
      user = await getMe();
    } catch {
      // An unreachable auth probe should not wedge the shell; treat as signed out.
      user = null;
    } finally {
      authChecked = true;
    }
    await loadSlots();
  }

  async function loadSlots() {
    if (
      config === null ||
      config.mode === 'hub' ||
      (config.auth_required && user === null)
    ) {
      slots = [];
      selectedSlot = null;
      summary = null;
      saveError = null;
      return;
    }

    try {
      const loaded = await getSlots();
      const wanted = selectedSlot ?? readStoredSlot();
      const next = loaded.find((slot) => slot.slot === wanted) ?? loaded[0] ?? null;

      slots = loaded;
      selectedSlot = next?.slot ?? null;
      if (next !== null) storeSlot(next.slot);
      await loadSummary();
    } catch (cause) {
      saveError = cause instanceof Error ? cause.message : String(cause);
    }
  }

  async function loadSummary() {
    const slot = selectedSlot;
    if (slot === null) {
      summary = null;
      summaryLoading = false;
      saveError = null;
      return;
    }

    const request = ++summaryRequest;
    if (summary?.slot !== slot) summary = null;
    summaryLoading = true;
    saveError = null;

    try {
      const detail = await getSlot(slot);
      if (request !== summaryRequest) return;
      summary = detail;
    } catch (cause) {
      if (request !== summaryRequest) return;
      summary = null;
      saveError = cause instanceof Error ? cause.message : String(cause);
    } finally {
      if (request === summaryRequest) summaryLoading = false;
    }
  }

  function selectSlot(slot: number) {
    if (slot === selectedSlot) return;
    selectedSlot = slot;
    storeSlot(slot);
    void loadSummary();
  }

  function handleAuthenticated(authenticated: User) {
    user = authenticated;
    authChecked = true;
    page = 'dashboard';
    void loadSlots();
  }

  async function handleLogout() {
    try {
      await logout();
    } catch {
      // Even if the request fails the session may already be gone; drop local state.
    }
    user = null;
    slots = [];
    selectedSlot = null;
    summary = null;
    saveError = null;
    await loadSlots();
  }

  onMount(() => {
    void loadConfig();
    retryTimer = setInterval(() => {
      if (config === null) void loadConfig();
    }, 5000);
    return () => {
      if (retryTimer !== null) clearInterval(retryTimer);
    };
  });
</script>

{#if showAuthGate}
  <Auth
    modeLabel={modeLabel}
    registrationOpen={config?.registration_open ?? true}
    onauthenticated={handleAuthenticated}
  />
{:else}
<div class="app-shell">
  <aside class="sidebar">
    <div class="brand">
      <div class="brand-mark" aria-hidden="true">◭</div>
      <div class="brand-text">
        <span class="brand-title">Wayfinder Slate</span>
        <span class="brand-subtitle">Breath of the Wild</span>
      </div>
    </div>

    <nav class="nav" aria-label="Primary">
      {#each NAV as item (item.id)}
        <button
          type="button"
          class="nav-item"
          class:active={page === item.id}
          aria-current={page === item.id ? 'page' : undefined}
          onclick={() => (page = item.id)}
        >
          <span class="nav-icon" aria-hidden="true">{item.icon}</span>
          <span class="nav-label">{item.label}</span>
        </button>
      {/each}
    </nav>

    <div class="sidebar-spacer"></div>

    {#if user !== null}
      <div class="account-card">
        <div class="account-identity">
          <span class="account-name" title={user.username}>{user.display_name}</span>
          <span class="account-role">{user.role === 'admin' ? 'Administrator' : 'Player'}</span>
        </div>
        <button
          type="button"
          class="logout-button"
          onclick={() => void handleLogout()}
        >
          Sign out
        </button>
      </div>
    {/if}

    <div class="sidebar-status">
      <div class="status-row">
        <span class="status-label">Mode</span>
        <span class="status-value">{modeLabel}</span>
      </div>
      <div class="status-row">
        <span class="status-label">Server</span>
        <span class="status-value">
          <span class="dot" class:online={config !== null}></span>
          {config !== null ? 'connected' : 'offline'}
        </span>
      </div>
      <div class="status-row">
        <span class="status-label">Version</span>
        <span class="status-value">{config?.version ?? '—'}</span>
      </div>
    </div>
  </aside>

  <div class="workspace">
    <header class="topbar">
      <div>
        <h1 class="topbar-title">{meta.title}</h1>
        <p class="topbar-subtitle">{meta.subtitle}</p>
      </div>
      <div class="topbar-actions">
        {#if showSlotPicker}
          <label class="slot-picker">
            <span class="slot-picker-label">Save slot</span>
            <select
              value={selectedSlot === null ? '' : String(selectedSlot)}
              onchange={(event) =>
                selectSlot(Number((event.currentTarget as HTMLSelectElement).value))}
              disabled={summaryLoading}
            >
              {#each slots as slot (slot.slot)}
                <option value={slot.slot}>{slot.label}</option>
              {/each}
            </select>
          </label>
          <button
            type="button"
            class="refresh-button"
            onclick={() => void loadSlots()}
            disabled={summaryLoading}
            title="Refresh save slots"
            aria-label="Refresh save slots"
          >
            ↻
          </button>
        {/if}

        <span
          class="mode-badge"
          class:hub={config?.mode === 'hub'}
          class:lan={config?.mode === 'lan'}
        >
          <span class="dot" class:online={config !== null}></span>
          {modeLabel}
        </span>
      </div>
    </header>

    <main class="page-wrap" class:map-page={page === 'map'}>
      {#if page === 'dashboard'}
        <Dashboard {config} {slots} {summary} {saveError} loading={summaryLoading} />
      {:else if page === 'map'}
        <MapView
          mapAssets={config?.map_assets ?? false}
          slot={selectedSlot}
          position={summary?.stats.location
            ? { x: summary.stats.location.x, z: summary.stats.location.z }
            : null}
        />
      {:else if page === 'play-data'}
        <PlayData {config} {summary} {saveError} loading={summaryLoading} />
      {:else}
        <Setup {config} />
      {/if}
    </main>
  </div>
</div>
{/if}

{#if error !== null}
  <div class="toast-error" role="status">
    Cannot reach the Wayfinder Slate server: {error}. Retrying…
  </div>
{/if}
