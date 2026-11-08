<script lang="ts">
  import type { AppConfig, SlotDetail } from '../lib/api';
  import SaveStatsRow from '../components/SaveStatsRow.svelte';

  let {
    config,
    summary,
    saveError,
    loading,
  }: {
    config: AppConfig | null;
    summary: SlotDetail | null;
    saveError: string | null;
    loading: boolean;
  } = $props();

  const completion = $derived(summary?.completion ?? null);
  const isHub = $derived(config?.mode === 'hub');

  const planned = [
    { label: 'Korok seeds', hint: 'Collected out of 900' },
    { label: 'Shrines', hint: 'Completed out of 136' },
    { label: 'Towers', hint: 'Activated Sheikah towers' },
    { label: 'Memories', hint: 'Recovered picture memories' },
    { label: 'Locations', hint: 'Discovered named locations' },
    { label: 'Enemies', hint: 'Defeat counters and medals' },
    { label: 'Chests', hint: 'Opened treasure chests' },
  ];
</script>

<div class="page">
  <section class="section" style="margin-bottom: 18px">
    <div class="progress-row">
      <div class="progress-ring" style="--pct: {completion?.shrines.percentage ?? 0}">
        <span>{completion?.shrines.completed ?? '—'}</span>
      </div>
      <div class="progress-copy">
        <span class="eyebrow">Play data</span>
        <h3>
          {completion
            ? (summary?.label ?? 'Save loaded')
            : loading
              ? 'Reading save…'
              : 'No save loaded'}
        </h3>
        <p>
          {#if completion}
            Shrines: {completion.shrines.completed}/{completion.shrines.total} ({completion.shrines.percentage.toFixed(1)}%) ·
            Towers: {completion.towers.completed}/{completion.towers.total} ·
            Koroks: {completion.koroks.completed}/{completion.koroks.total} ·
            Memories: {completion.memories.completed}/{completion.memories.total} ·
            Chests: {completion.treasures.completed}/{completion.treasures.total}
          {:else if saveError}
            {saveError}
          {:else}
            Choose a save slot to see play time, wallet, hearts, stamina, and
            completion from the selected save.
          {/if}
        </p>
      </div>
    </div>
  </section>

  {#if saveError}
    <div class="notice error" role="status" style="margin-bottom: 18px">
      <strong>Save unavailable.</strong>
      <span>{saveError}</span>
    </div>
  {/if}

  <SaveStatsRow stats={summary?.stats ?? null} />

  <section class="grid" aria-label="Play data summary">
    {#if completion}
      <article class="stat-card">
        <div class="stat-label">Shrines</div>
        <div class="stat-value">{completion.shrines.completed}/{completion.shrines.total}</div>
        <div class="stat-hint">{completion.shrines.percentage.toFixed(1)}% complete</div>
      </article>
      <article class="stat-card">
        <div class="stat-label">Towers</div>
        <div class="stat-value">{completion.towers.completed}/{completion.towers.total}</div>
        <div class="stat-hint">{completion.towers.percentage.toFixed(1)}% complete</div>
      </article>
      <article class="stat-card">
        <div class="stat-label">Korok Seeds</div>
        <div class="stat-value">{completion.koroks.completed}/{completion.koroks.total}</div>
        <div class="stat-hint">{completion.koroks.percentage.toFixed(1)}% complete</div>
      </article>
      <article class="stat-card">
        <div class="stat-label">Memories</div>
        <div class="stat-value">{completion.memories.completed}/{completion.memories.total}</div>
        <div class="stat-hint">{completion.memories.percentage.toFixed(1)}% complete</div>
      </article>
      <article class="stat-card">
        <div class="stat-label">Locations</div>
        <div class="stat-value">{completion.locations.completed}/{completion.locations.total}</div>
        <div class="stat-hint">{completion.locations.percentage.toFixed(1)}% complete</div>
      </article>
      <article class="stat-card">
        <div class="stat-label">Enemies Defeated</div>
        <div class="stat-value">{completion.enemies.completed}/{completion.enemies.total}</div>
        <div class="stat-hint">{completion.enemies.percentage.toFixed(1)}% complete</div>
      </article>
      <article class="stat-card">
        <div class="stat-label">Treasure Chests</div>
        <div class="stat-value">{completion.treasures.completed}/{completion.treasures.total}</div>
        <div class="stat-hint">{completion.treasures.percentage.toFixed(1)}% complete</div>
      </article>
    {:else}
      {#each planned as item (item.label)}
        <article class="stat-card">
          <div class="stat-label">{item.label}</div>
          <div class="stat-value">—</div>
          <div class="stat-hint">{item.hint}</div>
        </article>
      {/each}
    {/if}
  </section>

  <div class="two-col">
    <section class="section">
      <div class="section-head">
        <div>
          <h2>Medals &amp; titles</h2>
          <p>Derived from flags the game already writes.</p>
        </div>
      </div>
      <p style="color: var(--text-soft); font-size: 13px">
        BotW records monster medals, shrine counts, Korok milestones, and quest
        completion in ordinary save flags. Wayfinder Slate will translate those into
        readable achievements and compare them with your own earlier snapshots — not
        an arbitrary score.
      </p>
      <div class="empty-state" style="min-height: 170px; margin-top: 16px">
        <div>
          <h3>Medals arrive next</h3>
          <p>The save already records these flags; readable titles are still to come.</p>
        </div>
      </div>
    </section>

    <section class="section">
      <div class="section-head">
        <div>
          <h2>{isHub ? 'Global play data' : 'Progress history'}</h2>
          <p>{isHub ? 'Opt-in and aggregate only.' : 'Your saves, over time.'}</p>
        </div>
      </div>
      {#if isHub}
        <p style="color: var(--text-soft); font-size: 13px">
          On a public hub, global statistics are strictly opt-in. Individual saves
          stay private by default; only aggregate metrics you choose to share are
          included.
        </p>
        <div class="notice" style="margin-top: 16px">
          <strong>Privacy first.</strong>
          <span>
            No position history, raw save, or account identifier is included in
            global statistics.
          </span>
        </div>
      {:else}
        <p style="color: var(--text-soft); font-size: 13px">
          Each save sync creates a lightweight snapshot. Wayfinder Slate will chart
          play time, shrines, Koroks, and rupees over days and weeks, so progress is
          visible rather than a single frozen total.
        </p>
        <div class="empty-state" style="min-height: 170px; margin-top: 16px">
          <div>
            <h3>No history yet</h3>
            <p>Snapshots begin when save ingestion lands.</p>
          </div>
        </div>
      {/if}
    </section>
  </div>
</div>
