import type { ButtonHTMLAttributes } from "react";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  /**
   * DESIGN.md §7.2: "primary" is ink, "default" the bordered secondary,
   * "ghost" transparent, "danger" the destructive outline. "record" is the
   * accent fill and belongs to the Record action only.
   */
  variant?: "default" | "primary" | "ghost" | "danger" | "record";
  /** Height 32 (compact), 40 (default) or 48 (prominent). */
  size?: "compact" | "default" | "prominent";
}

export function Button({ variant = "default", size = "default", className, type = "button", ...rest }: ButtonProps) {
  return (
    <button
      type={type}
      data-variant={variant}
      data-size={size === "default" ? undefined : size}
      className={className ? `ob-button ${className}` : "ob-button"}
      {...rest}
    />
  );
}
