<script lang="ts">
  import { onMount } from 'svelte';
  import { getConfig, type AppConfig } from './lib/api';
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
      subtitle: 'Shrines, towers, memories, and every unfinished path.',
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

  let page = $state<PageId>('dashboard');
  let config = $state<AppConfig | null>(null);
  let error = $state<string | null>(null);
  let retryTimer: ReturnType<typeof setInterval> | null = null;

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

  async function loadConfig() {
    try {
      config = await getConfig();
      error = null;
      if (retryTimer !== null) {
        clearInterval(retryTimer);
        retryTimer = null;
      }
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    }
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
      <span
        class="mode-badge"
        class:hub={config?.mode === 'hub'}
        class:lan={config?.mode === 'lan'}
      >
        <span class="dot" class:online={config !== null}></span>
        {modeLabel}
      </span>
    </header>

    <main class="page-wrap" class:map-page={page === 'map'}>
      {#if page === 'dashboard'}
        <Dashboard {config} />
      {:else if page === 'map'}
        <MapView mapAssets={config?.map_assets ?? false} />
      {:else if page === 'play-data'}
        <PlayData {config} />
      {:else}
        <Setup {config} />
      {/if}
    </main>
  </div>
</div>

{#if error !== null}
  <div class="toast-error" role="status">
    Cannot reach the Wayfinder Slate server: {error}. Retrying…
  </div>
{/if}
