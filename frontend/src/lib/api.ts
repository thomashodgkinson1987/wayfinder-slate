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
}

export interface MapObject {
  object_type: 'shrine' | 'tower' | 'korok' | 'memory' | 'location' | 'enemy';
  name: string;
  flag: string;
  x: number;
  z: number;
  dlc?: boolean;
  seed?: string;
  loc?: string;
  enemy_type?: string;
  icon?: string;
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

export async function getCompletion(slot?: number): Promise<CompletionSummary> {
  const params = slot !== undefined ? `?slot=${slot}` : '';
  const response = await fetch(`/api/completion${params}`, {
    headers: { accept: 'application/json' },
  });
  if (!response.ok) {
    throw new Error(`Server returned ${response.status}`);
  }
  return (await response.json()) as CompletionSummary;
}
