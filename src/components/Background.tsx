import "./Background.css";

/** Decorative full-window background; hidden from assistive technology. */
export function Background() {
  return <div className="background" aria-hidden="true" data-testid="background" />;
}
