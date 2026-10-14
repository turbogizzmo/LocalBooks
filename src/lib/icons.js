import { createElement } from 'react';
import {
  ArrowLeft,
  ArrowRight,
  BookOpen,
  BookOpenText,
  Bot,
  ChartColumn,
  CircleHelp,
  CirclePlus,
  CreditCard,
  Landmark,
  LayoutDashboard,
  Lock,
  ReceiptText,
  Search,
  Settings,
  ShieldAlert,
  ShieldCheck,
  Upload,
  Users,
  X,
} from 'lucide-react';

/**
 * One icon per accounting concept, so the same idea always looks the same across the sidebar,
 * toolbars and help. Add a concept here rather than importing lucide-react in a component.
 */
export const ICONS = {
  NewTransaction: CirclePlus,
  Dashboard: LayoutDashboard,
  Sales: ReceiptText,
  Expenses: CreditCard,
  Employees: Users,
  Banking: Landmark,
  Accounting: BookOpen,
  Reports: ChartColumn,
  Settings,
  HelpCenter: CircleHelp,
  AuditTrail: ShieldAlert,
  Integrity: ShieldCheck,
  Import: Upload,
  ClosingDate: Lock,
  AiAccess: Bot,
  Search,
  Close: X,
  Go: ArrowRight,
  Back: ArrowLeft,
  Guide: BookOpenText,
};

/** The single size and stroke weight for every icon: compact, thin-line, high-density. */
export const ICON_SIZE = 16;
export const ICON_STROKE = 1.75;

/** Renders a registered icon at the standard size and stroke. Decorative unless given a `title`. */
export function Icon({ name, className = '', title, ...rest }) {
  const Glyph = ICONS[name];
  if (!Glyph) throw new Error(`Unknown icon "${name}". Register it in src/lib/icons.js.`);
  return createElement(Glyph, {
    size: ICON_SIZE,
    strokeWidth: ICON_STROKE,
    className: `flex-none ${className}`.trim(),
    'aria-hidden': title ? undefined : true,
    'aria-label': title,
    role: title ? 'img' : undefined,
    ...rest,
  });
}
