// Renders the engine's note Markdown, a small known subset: #/## headings,
// "- " bullets, **bold** lines, *italic* lines and "> " notices. Kept in-house
// so no third-party HTML renderer touches clinical text, and no HTML is
// injected (everything goes through React text nodes).
import type { ReactNode } from "react";

function inline(text: string): ReactNode[] {
  // **bold**, *italic* and [1, 2] evidence citations.
  const parts = text.split(/(\*\*[^*]+\*\*|\*[^*]+\*|\[\d+(?:, \d+)*\])/g).filter(Boolean);
  return parts.map((part, i) => {
    if (part.startsWith("**") && part.endsWith("**")) return <strong key={i}>{part.slice(2, -2)}</strong>;
    if (part.startsWith("*") && part.endsWith("*")) return <em key={i}>{part.slice(1, -1)}</em>;
    if (/^\[\d+(?:, \d+)*\]$/.test(part))
      return (
        <span key={i} className="cite" title="Transcript segment(s) this comes from">
          {part}
        </span>
      );
    return part;
  });
}

export function NoteMarkdown({ source }: { source: string }) {
  const blocks: ReactNode[] = [];
  let list: ReactNode[] = [];
  const flush = () => {
    if (list.length) blocks.push(<ul key={`ul${blocks.length}`}>{list}</ul>);
    list = [];
  };
  source.split("\n").forEach((line, i) => {
    if (line.startsWith("- ")) {
      list.push(<li key={i}>{inline(line.slice(2))}</li>);
      return;
    }
    flush();
    if (line.startsWith("# ")) blocks.push(<h1 key={i}>{inline(line.slice(2))}</h1>);
    else if (line.startsWith("## ")) blocks.push(<h2 key={i}>{inline(line.slice(3))}</h2>);
    else if (line.startsWith("> ")) blocks.push(<p key={i} className="ob-notice">{inline(line.slice(2))}</p>);
    else if (line.trim()) blocks.push(<p key={i}>{inline(line)}</p>);
  });
  flush();
  return <div className="note">{blocks}</div>;
}
