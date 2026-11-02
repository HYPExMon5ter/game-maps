// MapLibre 6 runs tile/GeoJSON work in an ES-module worker shipped as a separate
// file. Let Vite bundle it and point MapLibre at the emitted URL.
import { setWorkerUrl } from 'maplibre-gl'
import workerUrl from 'maplibre-gl/dist/maplibre-gl-worker.mjs?worker&url'

setWorkerUrl(workerUrl)
