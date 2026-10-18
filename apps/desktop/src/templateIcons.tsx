import {
  ArrowsClockwise,
  Bandaids,
  Bed,
  Bird,
  Bone,
  Brain,
  CalendarCheck,
  Cat,
  ChatText,
  ClipboardText,
  Cow,
  Dog,
  Drop,
  Ear,
  Eye,
  Fish,
  Flask,
  FlowerLotus,
  Hand,
  HandHeart,
  Heartbeat,
  Horse,
  Hospital,
  HouseLine,
  Microscope,
  Notepad,
  PawPrint,
  Phone,
  Pill,
  Rabbit,
  Receipt,
  Scissors,
  ShieldCheck,
  Siren,
  Stethoscope,
  Syringe,
  TestTube,
  Tooth,
  UsersThree,
  type Icon,
} from "@phosphor-icons/react";
import type { Template, TemplateSummary } from "./engine";

/**
 * Icons for kinds of visit (templates). Built-in templates name theirs in
 * their YAML (`icon: stethoscope`); a team picks one in the builder.
 * Phosphor, regular weight (DESIGN.md §5).
 */
export const TEMPLATE_ICONS: Record<string, Icon> = {
  stethoscope: Stethoscope,
  "arrows-clockwise": ArrowsClockwise,
  "calendar-check": CalendarCheck,
  phone: Phone,
  flask: Flask,
  "users-three": UsersThree,
  "clipboard-text": ClipboardText,
  siren: Siren,
  tooth: Tooth,
  scissors: Scissors,
  hand: Hand,
  microscope: Microscope,
  bird: Bird,
  horse: Horse,
  cow: Cow,
  "flower-lotus": FlowerLotus,
  "house-line": HouseLine,
  "chat-text": ChatText,
  receipt: Receipt,
  "hand-heart": HandHeart,
  heartbeat: Heartbeat,
  dog: Dog,
  cat: Cat,
  rabbit: Rabbit,
  fish: Fish,
  "paw-print": PawPrint,
  syringe: Syringe,
  pill: Pill,
  bandaids: Bandaids,
  eye: Eye,
  ear: Ear,
  bone: Bone,
  brain: Brain,
  bed: Bed,
  hospital: Hospital,
  drop: Drop,
  "test-tube": TestTube,
  notepad: Notepad,
  "shield-check": ShieldCheck,
};

/** The icon for a template; record notes default to a clipboard, client documents to a speech bubble. */
export function iconFor(
  t: { icon?: string; document?: Template["document"] | TemplateSummary["document"] } | null | undefined,
): Icon {
  const named = t?.icon ? TEMPLATE_ICONS[t.icon] : undefined;
  if (named) return named;
  return t?.document === "client" ? ChatText : ClipboardText;
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

/** An animal's icon by species key (patients list and pages); a paw when unknown. */
const SPECIES_ICONS: Record<string, Icon> = {
  canine: Dog,
  feline: Cat,
  equine: Horse,
  bovine: Cow,
  small_ruminant: Cow,
  porcine: PawPrint,
  avian: Bird,
  reptile: PawPrint,
  rabbit: Rabbit,
  rodent: Rabbit,
  ferret: PawPrint,
};

export function SpeciesIcon({ species, size = 20 }: { species?: string | null; size?: number }) {
  const I = (species && SPECIES_ICONS[species]) || PawPrint;
  return <I size={size} aria-hidden />;
}

/** The common visit types, shown first as tiles (in this order). */
export const COMMON_TYPES = ["sick-visit", "recheck", "wellness", "callback", "results", "rounds"];
