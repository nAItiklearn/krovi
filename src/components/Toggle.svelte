<script lang="ts">
  let {
    checked = $bindable(false),
    label,
    onchange,
    disabled = false,
    locked = false,
  }: {
    checked?: boolean;
    label: string;
    onchange?: (v: boolean) => void;
    disabled?: boolean;
    /** shown, announced, focusable, but fixed: for guarantees the app makes (e.g. privacy) */
    locked?: boolean;
  } = $props();

  function flip() {
    if (locked) return;
    checked = !checked;
    onchange?.(checked);
  }
</script>

<button
  class="toggle"
  class:locked
  type="button"
  role="switch"
  aria-checked={checked}
  aria-readonly={locked || undefined}
  aria-label={label}
  {disabled}
  onclick={flip}></button>

<style>
  .toggle:disabled {
    opacity: 0.4;
    pointer-events: none;
  }
  .toggle.locked {
    cursor: default;
  }
  .toggle.locked:active::after {
    width: 18px;
  }
</style>
