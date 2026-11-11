// Symbols drawn by the app (`builtin:<name>` icons). They ship with the app, so they don't
// depend on what a map pack has, and they're made to read at a glance on a small badge.

import { Anchor, Apple, Archive, Backpack, BedDouble, Bomb, BookOpen, Car, Clapperboard, Crosshair, Crown, DoorClosed, DraftingCompass, Fish, Flag, FlaskConical, Gem, Hammer, KeyRound, Layers, Leaf, Lock, Map as MapGlyph, MapPin, Mountain, Package, Pickaxe, Radiation, RadioTower, Sailboat, Scale, Shirt, Skull, Sword, Tent, TreePalm, UserRound, Users, Utensils, Warehouse, Wrench, type LucideIcon } from 'lucide-react'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'

const GLYPHS: Record<string, LucideIcon> = {
  ammo: Bomb,
  anchor: Anchor,
  backpack: Backpack,
  bed: BedDouble,
  blueprint: DraftingCompass,
  book: BookOpen,
  box: Package,
  bunker: DoorClosed,
  car: Car,
  cave: Mountain,
  chest: Archive,
  crate: Warehouse,
  crown: Crown,
  film: Clapperboard,
  fish: Fish,
  flag: Flag,
  food: Apple,
  friends: Users,
  gem: Gem,
  hammer: Hammer,
  hazard: Radiation,
  island: TreePalm,
  key: KeyRound,
  layers: Layers,
  leaf: Leaf,
  lock: Lock,
  map: MapGlyph,
  mod: Crosshair,
  person: UserRound,
  pickaxe: Pickaxe,
  pin: MapPin,
  radio: RadioTower,
  research: FlaskConical,
  sailboat: Sailboat,
  shirt: Shirt,
  skull: Skull,
  sword: Sword,
  tent: Tent,
  trade: Scale,
  utensils: Utensils,
  wrench: Wrench,
}

const urls = new Map<string, string>()

/** A symbol as a white image, or null if there's no symbol of that name. */
export function glyphUrl(name: string): string | null {
  const glyph = GLYPHS[name]
  if (!glyph) return null
  let url = urls.get(name)
  if (!url) {
    const svg = renderToStaticMarkup(createElement(glyph, { size: 96, color: '#ffffff', strokeWidth: 2.4 }))
    url = `data:image/svg+xml,${encodeURIComponent(svg)}`
    urls.set(name, url)
  }
  return url
}

/** Every symbol's name. */
export const GLYPH_NAMES = Object.keys(GLYPHS)
