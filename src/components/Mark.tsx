/** The hearth-arch mark (mono variant, colored via currentColor). */
export function Mark({ size = 18 }: { size?: number }) {
  return (
    <svg viewBox="0 0 64 64" width={size} height={size} role="img" aria-label="Bakehouse">
      <defs>
        <mask id="bh-racks-mono">
          <rect width="64" height="64" fill="#fff" />
          <rect x="20" y="22" width="24" height="4.5" rx="2.25" fill="#000" />
          <rect x="17.5" y="33" width="29" height="4.5" rx="2.25" fill="#000" />
          <rect x="17.5" y="44" width="29" height="4.5" rx="2.25" fill="#000" />
        </mask>
      </defs>
      <path
        d="M12 51V32a20 20 0 0 1 40 0v19a4 4 0 0 1-4 4H16a4 4 0 0 1-4-4Z"
        fill="currentColor"
        mask="url(#bh-racks-mono)"
      />
    </svg>
  );
}
