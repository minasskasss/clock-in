import { useTranslation } from "react-i18next";
import "./App.css";
import { Background } from "./components/Background";
import { QuickMenu } from "./components/QuickMenu";
import { useTheme } from "./theme/useTheme";

const STATUS_SAMPLES = [
  { key: "pending", className: "status-row--pending" },
  { key: "due", className: "status-row--due" },
  { key: "checkedIn", className: "status-row--in" },
  { key: "left", className: "status-row--left" },
  { key: "shouldHaveLeft", className: "status-row--late-out" },
] as const;

export default function App() {
  const { t } = useTranslation();
  const [theme, setTheme] = useTheme();

  return (
    <>
      <Background />
      <div className="app">
        <header className="app__header">
          <h1 className="app__title">{t("app.name")}</h1>
          <QuickMenu theme={theme} onThemeChange={setTheme} />
        </header>
        <main className="app__main">
          <section className="card">
            <h2 className="card__title">{t("placeholder.title")}</h2>
            <p className="card__body">{t("placeholder.body")}</p>
            <div className="legend">
              <h3 className="legend__title">{t("placeholder.legendTitle")}</h3>
              <ul className="legend__list">
                {STATUS_SAMPLES.map(({ key, className }) => (
                  <li key={key} className={`status-row ${className}`}>
                    <span className="status-row__label">{t(`status.${key}`)}</span>
                  </li>
                ))}
              </ul>
            </div>
          </section>
        </main>
      </div>
    </>
  );
}
