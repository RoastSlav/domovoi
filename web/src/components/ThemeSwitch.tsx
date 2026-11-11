import { useTheme, type Theme } from "../design/theme";
import { SegmentedControl, type Segment } from "./SegmentedControl";

const OPTIONS: readonly Segment<Theme>[] = [
  { value: "system", label: "System", icon: "monitor" },
  { value: "light", label: "Light", icon: "sun" },
  { value: "dark", label: "Dark", icon: "moon" },
];

/** Picks the colour theme: follow the device, or always light or dark. */
export function ThemeSwitch() {
  const { theme, setTheme } = useTheme();
  return (
    <SegmentedControl label="Theme" options={OPTIONS} value={theme} onChange={setTheme} stretch />
  );
}
