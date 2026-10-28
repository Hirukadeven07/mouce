import type { InputHTMLAttributes } from "react";

/** An on/off switch. It is a real checkbox underneath, announced as a switch. */
export function Switch(props: Omit<InputHTMLAttributes<HTMLInputElement>, "type" | "role">) {
  const { className, ...rest } = props;
  return <input {...rest} type="checkbox" role="switch" className={"switch" + (className ? " " + className : "")} />;
}
