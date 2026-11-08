<script lang="ts">
  import { onMount } from 'svelte';
  import type { AppConfig } from '../lib/api';
  import { getCompletion, type CompletionSummary } from '../lib/api';

  let { config }: { config: AppConfig | null } = $props();

  let completion = $state<CompletionSummary | null>(null);

  onMount(() => {
    void loadCompletion();
  });

  async function loadCompletion() {
    try {
      completion = await getCompletion();
    } catch {
      // Ignore errors
    }
  }

  const planned = [
    { label: 'Play time', hint: 'From the save play-report counters' },
    { label: 'Rupees', hint: 'Current wallet and lifetime totals' },
    { label: 'Korok seeds', hint: 'Collected out of 900' },
    { label: 'Shrines', hint: 'Completed out of 120 (plus DLC)' },
    { label: 'Towers', hint: 'Activated Sheikah towers' },
    { label: 'Memories', hint: 'Recovered picture memories' },
    { label: 'Divine Beasts', hint: 'Main quest progress' },
    { label: 'Enemies', hint: 'Defeat counters and medals' },
  ];

  const isHub = $derived(config?.mode === 'hub');
</script>

<div class="page">
  <section class="section" style="margin-bottom: 18px">
    <div class="progress-row">
      <div class="progress-ring" style="--pct: {completion?.shrines.percentage ?? 0}">
        <span>{completion?.shrines.completed ?? '—'}</span>
      </div>
      <div class="progress-copy">
        <span class="eyebrow">Play data</span>
        <h3>{completion ? 'Save loaded' : 'No snapshots yet'}</h3>
        <p>
          {#if completion}
            Shrines: {completion.shrines.completed}/{completion.shrines.total} ({completion.shrines.percentage.toFixed(1)}%) · 
            Towers: {completion.towers.completed}/{completion.towers.total} · 
            Koroks: {completion.koroks.completed}/{completion.koroks.total} · 
            Memories: {completion.memories.completed}/{completion.memories.total}
          {:else}
            Once Wayfinder Slate reads a save, this page will show total play time,
            completion, medals, and a progress timeline built from stored snapshots.
          {/if}
        </p>
      </div>
    </div>
  </section>

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
          <h2>Medals & titles</h2>
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
          <h3>Waiting for a save snapshot</h3>
          <p>Medals and titles will appear here automatically.</p>
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