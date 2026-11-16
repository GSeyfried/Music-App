import type { ReactNode } from "react";

interface PanelProps {
  title: string;
  /** Marks UI that is a placeholder for a later milestone. */
  milestone?: string;
  actions?: ReactNode;
  className?: string;
  children: ReactNode;
}

export function Panel({ title, milestone, actions, className, children }: PanelProps) {
  return (
    <section className={`panel ${className ?? ""}`}>
      <header className="panel-head">
        <h2>{title}</h2>
        {milestone && <span className="tag">{milestone}</span>}
        {actions && <div className="panel-actions">{actions}</div>}
      </header>
      <div className="panel-body">{children}</div>
    </section>
  );
}

export function Placeholder({ children }: { children: ReactNode }) {
  return <div className="placeholder">{children}</div>;
}
