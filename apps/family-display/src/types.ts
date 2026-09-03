export type DisplayMode = "today" | "week" | "month" | "morning" | "evening";
export type DisplayKind = "task" | "appointment" | "reminder" | "notice";
export type DisplayCategory = "work" | "family" | "school" | "creche" | "kids" | "personal" | "home";

export interface DisplayItem {
  displayId: string;
  title: string;
  kind: DisplayKind;
  category: DisplayCategory;
  startsAt?: string;
  endsAt?: string;
  detailLevel: "full" | "generic";
}

export interface DisplaySnapshot {
  schemaVersion: 1;
  generatedAt: string;
  mode: DisplayMode;
  items: DisplayItem[];
}
