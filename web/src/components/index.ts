/**
 * The component kit. Page code imports from here and composes these; it does not style its
 * own buttons, pills, cards, dialogs or tooltips. See web/DESIGN.md for when to use each.
 */

// Status
export { BadgePill, StatusPill } from "./StatusPill";
export { Tag } from "./Tag";
export { Count } from "./Count";
export { toneStripe } from "./ToneStripe";

// Messages and feedback
export { InlineError, InlineMessage } from "./InlineMessage";
export { Banner } from "./Banner";
export { ErrorDetail } from "./ErrorDetail";
export { ToastProvider, useToast, type ToastAction, type ToastApi, type ToastInput } from "./Toast";
export { useActionFeedback, type ActionFeedback, type FeedbackMessages } from "./useActionFeedback";

// Hints
export { Tooltip } from "./Tooltip";
export { InfoTip } from "./InfoTip";
export { DocsLink } from "./DocsLink";
export { Kbd } from "./Kbd";

// Buttons
export { Button, LinkButton, type ButtonProps, type ButtonVariant } from "./Button";
export { IconButton } from "./IconButton";
export { CopyButton } from "./CopyButton";

// Loading, empty, error
export { Loading } from "./Loading";
export { Skeleton } from "./Skeleton";
export { Spinner } from "./Spinner";
export { Progress } from "./Progress";
export { LoadError } from "./LoadError";
export { EmptyState } from "./EmptyState";
export { QueryBoundary } from "./QueryBoundary";

// Layout
export { Cluster, Grid, Stack, type Gap } from "./Stack";
export { PageHeader } from "./PageHeader";
export { Section } from "./Section";
export { Card } from "./Card";
export { Text } from "./Text";

// Data
export { DataTable, type Column, type TableSort } from "./DataTable";
export { KeyValueList, type KeyValue } from "./KeyValueList";
export { FilterChips, type Chip } from "./FilterChips";
export { Tabs, type Tab } from "./Tabs";
export { RelativeTime } from "./RelativeTime";
export { Timeline, TIMELINE_PREVIEW, type TimelineItem } from "./Timeline";
export { Countdown, formatRemaining, useRemaining } from "./Countdown";
export { CopyBlock } from "./CopyBlock";
export { Markdown } from "./Markdown";

// Forms
export { Field, fieldIdForPath } from "./Field";
export { Checkbox, NumberInput, Select, Switch, TextInput, Textarea } from "./inputs";
export { SegmentedControl, type Segment } from "./SegmentedControl";
export { FormErrorSummary } from "./FormErrorSummary";
export { SubmitBar } from "./SubmitBar";
export { ThemeSwitch } from "./ThemeSwitch";

// Overlays
export { Dialog, type DialogProps } from "./Dialog";
export { ConfirmDialog, type ConfirmDialogProps } from "./ConfirmDialog";
export { PasswordPrompt } from "./PasswordPrompt";
export { Menu, type MenuItem } from "./Menu";
export { OptionPicker, PICKER_FILTER_FROM, type PickerOption } from "./OptionPicker";
export { Popover } from "./Popover";

// Chat
export {
  ChatThread,
  MAX_HISTORY_TURNS,
  MAX_QUESTION_CHARS,
  type ChatThreadProps,
  type ChatTool,
  type ChatTurn,
} from "./ChatThread";

// Hooks
export { useDebounced } from "./useDebounced";
