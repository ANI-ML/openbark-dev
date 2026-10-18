import {
  Activity,
  Bandage,
  Bed,
  Bird,
  Bone,
  CalendarCheck,
  Cat,
  ClipboardList,
  Dog,
  Droplet,
  Ear,
  Eye,
  Fence,
  Fish,
  FlaskConical,
  Flower2,
  Hand,
  HandHeart,
  HeartPulse,
  Hospital,
  House,
  MessageSquareText,
  Microscope,
  NotepadText,
  PawPrint,
  Phone,
  Pill,
  Rabbit,
  Rat,
  Receipt,
  RefreshCw,
  Scissors,
  ShieldCheck,
  Siren,
  Stethoscope,
  Syringe,
  TestTube,
  Toothbrush,
  Tractor,
  Turtle,
  Users,
  type LucideIcon,
} from "lucide-react";
import type { Template, TemplateSummary } from "./engine";

/**
 * Icons for kinds of visit (templates). Built-in templates name theirs in
 * their YAML (`icon: stethoscope`); a team picks one in the builder, and
 * saved templates keep that name. The names predate the Lucide set
 * (DESIGN.md §6), so each old name maps to the closest Lucide icon; never
 * rename a key, only add. Lucide has no horse, cow or tooth: equine visits
 * show a paddock fence, production-animal visits a tractor, dental a
 * toothbrush.
 */
export const TEMPLATE_ICONS: Record<string, LucideIcon> = {
  stethoscope: Stethoscope,
  "arrows-clockwise": RefreshCw,
  "calendar-check": CalendarCheck,
  phone: Phone,
  flask: FlaskConical,
  "users-three": Users,
  "clipboard-text": ClipboardList,
  siren: Siren,
  tooth: Toothbrush,
  scissors: Scissors,
  hand: Hand,
  microscope: Microscope,
  bird: Bird,
  horse: Fence,
  cow: Tractor,
  "flower-lotus": Flower2,
  "house-line": House,
  "chat-text": MessageSquareText,
  receipt: Receipt,
  "hand-heart": HandHeart,
  heartbeat: HeartPulse,
  dog: Dog,
  cat: Cat,
  rabbit: Rabbit,
  fish: Fish,
  "paw-print": PawPrint,
  syringe: Syringe,
  pill: Pill,
  bandaids: Bandage,
  eye: Eye,
  ear: Ear,
  bone: Bone,
  bed: Bed,
  hospital: Hospital,
  drop: Droplet,
  "test-tube": TestTube,
  notepad: NotepadText,
  "shield-check": ShieldCheck,
  activity: Activity,
};

/**
 * Names that still resolve but are no longer offered in the builder.
 * "brain" (neurology) shows the activity trace: the brand never uses a
 * brain icon (DESIGN.md §6).
 */
const TEMPLATE_ICON_ALIASES: Record<string, LucideIcon> = {
  brain: Activity,
};

/** The icon for a template; record notes default to a clipboard, client documents to a speech bubble. */
export function iconFor(
  t: { icon?: string; document?: Template["document"] | TemplateSummary["document"] } | null | undefined,
): LucideIcon {
  const named = t?.icon ? (TEMPLATE_ICONS[t.icon] ?? TEMPLATE_ICON_ALIASES[t.icon]) : undefined;
  if (named) return named;
  return t?.document === "client" ? MessageSquareText : ClipboardList;
}

export function TemplateIcon({
  template,
  size = 20,
  className,
}: {
  template: { icon?: string; document?: Template["document"] | TemplateSummary["document"] } | null | undefined;
  size?: number;
  className?: string;
}) {
  const I = iconFor(template);
  return <I size={size} className={className} aria-hidden />;
}

/**
 * An animal's icon by species key (patients list and pages); a paw when
 * unknown. Lucide draws no hoofed animals, so horses, cattle, small
 * ruminants, pigs and ferrets take the paw rather than an object icon.
 */
const SPECIES_ICONS: Record<string, LucideIcon> = {
  canine: Dog,
  feline: Cat,
  avian: Bird,
  reptile: Turtle,
  rabbit: Rabbit,
  rodent: Rat,
};

export function SpeciesIcon({ species, size = 20 }: { species?: string | null; size?: number }) {
  const I = (species && SPECIES_ICONS[species]) || PawPrint;
  return <I size={size} aria-hidden />;
}

/** The common visit types, shown first as tiles (in this order). */
export const COMMON_TYPES = ["sick-visit", "recheck", "wellness", "callback", "results", "rounds"];
