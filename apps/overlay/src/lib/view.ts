// Which view this web page is: the overlay over the game (default) or the map window
// kept open on another monitor (`?view=window`, see `src-tauri/src/map_window.rs`).

export const IS_MAP_WINDOW = typeof window !== 'undefined' && new URLSearchParams(window.location.search).get('view') === 'window'
