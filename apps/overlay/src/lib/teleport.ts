import { useSettings } from '../settings/store'
import { useStore } from '../state/store'

/** Whether teleporting is offered at all (the player's choice). */
export function useTeleportOffered(): boolean {
  return useSettings((s) => s.ui.teleport)
}

/** Whether a teleport would work right now. */
export function useCanTeleport(): boolean {
  const connected = useStore((s) => s.status?.state === 'connected' && !!s.status.capabilities?.teleport)
  return useTeleportOffered() && connected
}
