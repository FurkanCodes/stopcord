import * as React from 'react';
import { Field as BaseField } from '@base-ui/react/field';
import { Input } from '@base-ui/react/input';
import { cx, type WithClassName } from '../cx';
import { controlFrame } from './Select';

export interface FieldProps {
  label: string;
  /** A hint under the control, in ink-2. Keep it to one line. */
  description?: React.ReactNode;
  /** Shown in red text under the control. Setting it marks the field invalid. */
  error?: React.ReactNode;
  name?: string;
  disabled?: boolean;
  className?: string;
  /** One control: TextInput, TextArea or Select. */
  children: React.ReactNode;
}

/** A label above one control, with an optional hint and error below. */
export function Field({ label, description, error, className, children, ...root }: FieldProps) {
  return (
    <BaseField.Root invalid={error ? true : undefined} className={cx('flex flex-col gap-1.5', className)} {...root}>
      <BaseField.Label className="text-body font-medium text-ink-2">{label}</BaseField.Label>
      {children}
      {description && <BaseField.Description className="m-0 text-meta text-ink-2">{description}</BaseField.Description>}
      {error && (
        <BaseField.Error match={true} className="text-meta text-needs-text">
          {error}
        </BaseField.Error>
      )}
    </BaseField.Root>
  );
}

export interface TextInputProps extends WithClassName<Input.Props> {
  /** Commands, branches, paths. */
  mono?: boolean;
}

/** A one-line text input. 34px, on the page ground, with an edge border. */
export const TextInput = React.forwardRef<HTMLElement, TextInputProps>(function TextInput(
  { mono = false, className, ...props },
  ref,
) {
  return (
    <Input
      ref={ref}
      // Commands, branches and paths must stay exactly as typed.
      {...(mono ? { autoCapitalize: 'off', autoCorrect: 'off', spellCheck: false } : null)}
      className={cx(
        controlFrame,
        'h-[34px] px-2.5 placeholder:text-ink-2 focus-visible:outline-offset-0',
        mono && 'font-mono text-meta',
        className,
      )}
      {...props}
    />
  );
});

export interface TextAreaProps extends WithClassName<Omit<BaseField.Control.Props, 'render'>> {
  rows?: number;
}

/** Long text such as the knowledge base or a rule file. Mono, on the terminal ground, resizes down. */
export const TextArea = React.forwardRef<HTMLElement, TextAreaProps>(function TextArea(
  { rows = 9, className, ...props },
  ref,
) {
  return (
    <BaseField.Control
      ref={ref}
      render={<textarea rows={rows} />}
      className={cx(
        controlFrame,
        'resize-y bg-term p-2.5 font-mono text-meta leading-relaxed placeholder:text-ink-2 focus-visible:outline-offset-0',
        className,
      )}
      {...props}
    />
  );
});
