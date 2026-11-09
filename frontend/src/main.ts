import { mount } from 'svelte';
import L from 'leaflet';
import 'leaflet/dist/leaflet.css';
import 'leaflet.markercluster/dist/MarkerCluster.css';
import 'leaflet.markercluster/dist/MarkerCluster.Default.css';
import './app.css';
import App from './App.svelte';

// leaflet.markercluster is a UMD plugin: it reads and augments a *global* `L`
// when its module evaluates. Leaflet is an ES module here, and static imports
// are hoisted, so loading the plugin statically runs before `L` exists and
// throws `ReferenceError: L is not defined` (blank page). Seed the global with
// the same Leaflet instance and load the plugin dynamically before mounting.
async function bootstrap() {
  (globalThis as typeof globalThis & { L?: typeof L }).L = L;
  await import('leaflet.markercluster');

  mount(App, {
    target: document.getElementById('app')!,
  });
}

void bootstrap();
