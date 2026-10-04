import "./Background.css";

/** Decorative full-window background; hidden from assistive technology. */
export function Background() {
  return (
    <div className="background" aria-hidden="true" data-testid="background">
      <div className="background__blob background__blob--1" />
      <div className="background__blob background__blob--2" />
      <div className="background__blob background__blob--3" />
      <div className="background__blob background__blob--4" />
      <div className="background__grain" />
    </div>
  );
}
