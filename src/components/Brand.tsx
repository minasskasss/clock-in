import { useTranslation } from "react-i18next";
import iconUrl from "../../assets/icon/icon.svg";
import { WORDMARK_ASPECT, WORDMARK_PATH, WORDMARK_VIEW_BOX } from "./wordmark";

const NAME_HEIGHT = 27;

/** The header mark: the app icon, "Clock In" in italic and the Greek subtitle. */
export function Brand() {
  const { t } = useTranslation();
  return (
    <span className="brand">
      <img className="brand__icon" src={iconUrl} alt="" width={36} height={36} />
      <span className="brand__text">
        <svg
          className="brand__name"
          viewBox={WORDMARK_VIEW_BOX}
          width={Math.round(NAME_HEIGHT * WORDMARK_ASPECT)}
          height={NAME_HEIGHT}
          role="img"
          aria-label={t("app.name")}
        >
          <path d={WORDMARK_PATH} />
        </svg>
        <span className="brand__subtitle">{t("app.subtitle")}</span>
      </span>
    </span>
  );
}
