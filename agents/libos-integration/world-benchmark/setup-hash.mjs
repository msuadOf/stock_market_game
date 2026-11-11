export async function setupHash(setup, subtle) {
  const bytes = new TextEncoder().encode(JSON.stringify(setup));
  const digest = await subtle.digest('SHA-256', bytes);
  return Array.from(new Uint8Array(digest), byte => byte.toString(16).padStart(2, '0')).join('');
}
