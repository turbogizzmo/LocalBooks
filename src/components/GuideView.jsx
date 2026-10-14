import { Fragment } from 'react';

/**
 * The step-by-step guides in docs/guide/*.md, bundled into the app so they work offline. The same
 * files are the user guide on GitHub.
 */
const SOURCES = import.meta.glob('../../docs/guide/*.md', { query: '?raw', import: 'default', eager: true });

export const GUIDES = Object.fromEntries(
  Object.entries(SOURCES).map(([path, text]) => {
    const id = path.split('/').pop().replace(/\.md$/, '');
    const title = text.match(/^#\s+(.+)$/m)?.[1] ?? id;
    return [id, { id, title, text }];
  }),
);

/** Guide ids in the order of the guide index (docs/guide/README.md). */
export const GUIDE_ORDER = [...(GUIDES.README?.text ?? '').matchAll(/\]\(([\w-]+)\.md\)/g)].map((m) => m[1]).filter((id) => GUIDES[id]);

/** Inline Markdown: **bold**, `code`, and [links](other-guide.md). */
function Inline({ text, onOpenGuide }) {
  const parts = [];
  const pattern = /\*\*(.+?)\*\*|`([^`]+)`|\[([^\]]+)\]\(([^)]+)\)/g;
  let last = 0;
  let m;
  while ((m = pattern.exec(text))) {
    if (m.index > last) parts.push(text.slice(last, m.index));
    if (m[1] !== undefined) parts.push(<strong key={m.index} className="font-semibold text-qb-ink">{m[1]}</strong>);
    else if (m[2] !== undefined) parts.push(<code key={m.index} className="rounded-xs bg-qb-canvas px-1 font-mono text-[11px]">{m[2]}</code>);
    else {
      const guide = m[4].match(/^([\w-]+)\.md$/)?.[1];
      parts.push(
        guide && GUIDES[guide] ? (
          <button key={m.index} type="button" onClick={() => onOpenGuide(guide)} className="font-semibold text-qb-green-active hover:underline">
            {m[3]}
          </button>
        ) : (
          <Fragment key={m.index}>{m[3]}</Fragment>
        ),
      );
    }
    last = pattern.lastIndex;
  }
  if (last < text.length) parts.push(text.slice(last));
  return parts;
}

/**
 * Parses the Markdown subset the guides use: headings, paragraphs, numbered and bulleted lists
 * (continuation lines and one level of nested bullets indented under an item), and > tip boxes.
 */
export function parseGuide(text) {
  const blocks = [];
  const lines = text.replace(/\r\n/g, '\n').split('\n');
  let i = 0;
  const isItem = (l) => /^(\d+\.|-)\s+/.test(l);
  while (i < lines.length) {
    const line = lines[i];
    if (!line.trim()) {
      i += 1;
    } else if (/^#{1,3}\s/.test(line)) {
      const level = line.match(/^#+/)[0].length;
      blocks.push({ type: 'heading', level, text: line.replace(/^#+\s+/, '') });
      i += 1;
    } else if (line.startsWith('>')) {
      const quote = [];
      while (i < lines.length && lines[i].startsWith('>')) quote.push(lines[i++].replace(/^>\s?/, ''));
      blocks.push({ type: 'tip', text: quote.join(' ') });
    } else if (isItem(line)) {
      const ordered = /^\d+\./.test(line);
      const items = [];
      while (i < lines.length && (isItem(lines[i]) || /^\s{2,}\S/.test(lines[i]) || (!lines[i].trim() && /^\s{2,}\S/.test(lines[i + 1] ?? '')))) {
        const l = lines[i];
        if (isItem(l) && /^\d+\./.test(l) === ordered) {
          items.push({ text: l.replace(/^(\d+\.|-)\s+/, ''), children: [], extra: [] });
        } else if (/^\s+-\s+/.test(l) && items.length) {
          items[items.length - 1].children.push(l.replace(/^\s+-\s+/, ''));
        } else if (/^\s{2,}\S/.test(l) && items.length) {
          const item = items[items.length - 1];
          if (item.children.length) item.children[item.children.length - 1] += ` ${l.trim()}`;
          else if (item.extra.length || lines[i - 1]?.trim() === '') item.extra.push(l.trim());
          else item.text += ` ${l.trim()}`;
        } else if (isItem(l)) {
          break;
        }
        i += 1;
      }
      blocks.push({ type: ordered ? 'steps' : 'bullets', items });
    } else {
      const para = [];
      while (i < lines.length && lines[i].trim() && !/^(#{1,3}\s|>|\d+\.\s|-\s)/.test(lines[i])) para.push(lines[i++].trim());
      blocks.push({ type: 'para', text: para.join(' ') });
    }
  }
  return blocks;
}

/** Renders one guide for the Help panel. */
export default function GuideView({ id, onOpenGuide }) {
  const guide = GUIDES[id];
  if (!guide) return <p className="px-3 py-3 text-qb-ink-muted">This guide is not available.</p>;
  const inline = (text) => <Inline text={text} onOpenGuide={onOpenGuide} />;
  return (
    <article className="flex flex-col gap-2 px-3 py-3 text-xs leading-relaxed text-qb-ink">
      {parseGuide(guide.text).map((b, n) => {
        if (b.type === 'heading') {
          if (b.level === 1) return <h3 key={n} className="text-lg font-semibold leading-tight">{b.text}</h3>;
          return <h4 key={n} className={`${b.level === 2 ? 'mt-2 text-md' : 'mt-1 text-sm'} font-semibold`}>{inline(b.text)}</h4>;
        }
        if (b.type === 'tip') {
          return (
            <div key={n} className="rounded-xs border-l-[3px] border-qb-green bg-qb-green-tint px-2 py-1.5">
              {inline(b.text)}
            </div>
          );
        }
        if (b.type === 'para') return <p key={n} className="text-qb-ink-muted">{inline(b.text)}</p>;
        const List = b.type === 'steps' ? 'ol' : 'ul';
        return (
          <List key={n} className="flex flex-col gap-1.5">
            {b.items.map((item, k) => (
              <li key={k} className="flex gap-2">
                {b.type === 'steps' ? (
                  <span className="mt-[1px] flex h-4 w-4 flex-none items-center justify-center rounded-full bg-qb-nav text-[10px] font-semibold text-white">{k + 1}</span>
                ) : (
                  <span className="mt-[7px] h-1 w-1 flex-none rounded-full bg-qb-ink-muted" />
                )}
                <div className="min-w-0">
                  {inline(item.text)}
                  {item.children.length > 0 && (
                    <ul className="mt-1 flex flex-col gap-1 border-l border-qb-border pl-2">
                      {item.children.map((c, j) => <li key={j}>{inline(c)}</li>)}
                    </ul>
                  )}
                  {item.extra.map((e, j) => (
                    <div key={j} className="mt-1">{inline(e)}</div>
                  ))}
                </div>
              </li>
            ))}
          </List>
        );
      })}
    </article>
  );
}
