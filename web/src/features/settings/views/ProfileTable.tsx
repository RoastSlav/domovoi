import { Cluster, DataTable, InfoTip, type Column } from "../../../components";
import {
  CATEGORIES,
  CATEGORY_META,
  PRESENTATION,
  PRESENTATION_WORDS,
  PROFILE_IDS,
  PROFILE_META,
  type Category,
} from "../../../design/visibility";

const COLUMNS: Column<Category>[] = [
  {
    key: "category",
    label: "Category",
    cell: (category) => (
      <Cluster gap={1}>
        <span>{CATEGORY_META[category].label}</span>
        <InfoTip title={CATEGORY_META[category].label}>{CATEGORY_META[category].hint}</InfoTip>
      </Cluster>
    ),
  },
  ...PROFILE_IDS.map((profile): Column<Category> => ({
    key: profile,
    label: PROFILE_META[profile].label,
    cell: (category) => PRESENTATION_WORDS[PRESENTATION[profile][category]],
  })),
];

/** What each detail level does with each category, read from the table the screens use. */
export function ProfileTable() {
  return (
    <DataTable
      caption="What each detail level shows"
      columns={COLUMNS}
      rows={CATEGORIES}
      rowKey={(category) => category}
    />
  );
}
