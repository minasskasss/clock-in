import "i18next";
import type el from "./el.json";

// Type-checks translation keys against the Greek (default) resource.
declare module "i18next" {
  interface CustomTypeOptions {
    defaultNS: "translation";
    resources: {
      translation: typeof el;
    };
  }
}
