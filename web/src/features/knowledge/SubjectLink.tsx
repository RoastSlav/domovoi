import { Link } from "react-router";
import { subjectLabel, type SubjectNames } from "./subjects";

/** A subject in words, as a link to its host or entity when it has one; always plain text. */
export function SubjectLink({ subject, names }: { names: SubjectNames; subject: string }) {
  const label = subjectLabel(subject, names);
  return label.to ? <Link to={label.to}>{label.text}</Link> : <span>{label.text}</span>;
}
