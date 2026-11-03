// The settings panel over the full map (see `settings/SettingsView`).

import { useCallback } from 'react'
import { SettingsView } from '../settings/SettingsView'
import { useStore } from '../state/store'

export function SettingsPanel({ onClose }: { onClose: () => void }) {
  const gameId = useStore((s) => s.game?.id ?? null)
  const notify = useStore((s) => s.notify)
  const onError = useCallback((message: string) => notify(message, 'error'), [notify])
  return (
    <div className="glass pointer-events-auto h-[min(600px,100%)] w-[620px] max-w-full overflow-hidden rounded-2xl">
      <SettingsView window="map" initialGameId={gameId} onClose={onClose} onError={onError} />
    </div>
  )
}
