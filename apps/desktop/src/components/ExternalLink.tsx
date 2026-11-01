import type { ReactNode } from "react";
import * as engine from "../engine";

/** NVIDIA's licence for the libraries in the GPU speech pack (os_app::links::NVIDIA_LICENCE). */
export const NVIDIA_LICENCE = "https://docs.nvidia.com/deeplearning/cudnn/latest/reference/eula.html";

/**
 * A link to a web page, opened in the default browser. In web mode it is an
 * ordinary link to a new tab. On the desktop the main window may not open
 * addresses itself (capabilities/default.json grants no opener, so a plain
 * target="_blank" link does nothing there): the click asks the app to open
 * the page it names, and the engine builds the address. Preventing the
 * default here also stops the opener plugin's own click handler, which
 * would be refused.
 */
export function ExternalLink({
  href,
  page,
  className,
  children,
}: {
  href: string;
  page: engine.WebPage;
  className?: string;
  children: ReactNode;
}) {
  return (
    <a
      className={className}
      href={href}
      target="_blank"
      rel="noreferrer"
      onClick={(e) => {
        if (!engine.isDesktop) return;
        e.preventDefault();
        engine.openWebPage(page).catch(() => {});
      }}
    >
      {children}
    </a>
  );
}
