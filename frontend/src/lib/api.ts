export type DeploymentMode = 'local' | 'lan' | 'hub';

export interface AppConfig {
  mode: DeploymentMode;
  version: string;
  live: boolean;
  save_dir: string | null;
  control_addr: string | null;
  map_assets: boolean;
}

export interface CompletionCount {
  total: number;
  completed: number;
  percentage: number;
}

export interface CompletionSummary {
  shrines: CompletionCount;
  towers: CompletionCount;
  koroks: CompletionCount;
  memories: CompletionCount;
  locations: CompletionCount;
  enemies: CompletionCount;
  treasures: CompletionCount;
}

export interface HeartStats {
  current: number;
  max: number;
}

export interface StaminaStats {
  current: number;
  max: number;
}

export interface SaveLocation {
  map: string;
  map_type: string;
  x: number;
  y: number;
  z: number;
}

export interface SaveStats {
  play_time_seconds: number;
  rupees: number;
  hearts: HeartStats;
  stamina: StaminaStats;
  location: SaveLocation;
}

export interface SaveCaption {
  saved_at: number;
  location: string;
  map_area: string;
}

export interface SlotSummary extends SaveCaption {
  slot: number;
  label: string;
  caption_url: string;
}

export interface SlotDetail extends SlotSummary {
  stats: SaveStats;
  completion: CompletionSummary;
}

export type MapObjectType =
  | 'shrine'
  | 'tower'
  | 'korok'
  | 'memory'
  | 'location'
  | 'enemy'
  | 'treasure';

export interface MapObject {
  /** Save flag name. Treasure chests are hash-addressed and carry an `id` instead. */
  flag?: string;
  x: number;
  z: number;
  name?: string;
  dlc?: boolean;
  seed?: string;
  loc?: string;
  type?: string;
  match_name?: string;
  icon?: string;
  id?: string;
  group?: string;
}

export interface MapObjectsDataset {
  version: number;
  source: Record<string, string>;
  shrines: MapObject[];
  towers: MapObject[];
  koroks: MapObject[];
  memories: MapObject[];
  locations: MapObject[];
  enemies: MapObject[];
  treasures: MapObject[];
}

export async function getConfig(): Promise<AppConfig> {
  const response = await fetch('/api/config', {
    headers: { accept: 'application/json' },
  });
  if (!response.ok) {
    throw new Error(`Server returned ${response.status}`);
  }
  return (await response.json()) as AppConfig;
}

export async function getMapObjects(): Promise<MapObjectsDataset> {
  const response = await fetch('/api/map-objects', {
    headers: { accept: 'application/json' },
  });
  if (!response.ok) {
    throw new Error(`Server returned ${response.status}`);
  }
  return (await response.json()) as MapObjectsDataset;
}

export async function getSlots(): Promise<SlotSummary[]> {
  const response = await fetch('/api/slots', {
    headers: { accept: 'application/json' },
  });
  if (!response.ok) {
    throw new Error(`Save slots could not be loaded (server returned ${response.status}).`);
  }
  return (await response.json()) as SlotSummary[];
}

export async function getSlot(slot: number): Promise<SlotDetail> {
  const response = await fetch(`/api/slots/${slot}`, {
    headers: { accept: 'application/json' },
  });
  if (response.status === 404) {
    throw new Error('Save slot not found.');
  }
  if (response.status === 422) {
    throw new Error('The selected save file could not be parsed.');
  }
  if (!response.ok) {
    throw new Error(`Save slot could not be loaded (server returned ${response.status}).`);
  }
  return (await response.json()) as SlotDetail;
}
