import type { SVGProps } from "react";

type IconProps = SVGProps<SVGSVGElement>;

function Icon({ children, ...props }: IconProps) {
  return (
    <svg
      width={16}
      height={16}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      {...props}
    >
      {children}
    </svg>
  );
}

export const Icons = {
  chevronRight: (props: IconProps) => (
    <Icon {...props}>
      <path d="m9 18 6-6-6-6" />
    </Icon>
  ),
  chevronDown: (props: IconProps) => (
    <Icon {...props}>
      <path d="m6 9 6 6 6-6" />
    </Icon>
  ),
};
