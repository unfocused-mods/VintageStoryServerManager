const adjectives = ["quiet", "mossy", "copper", "silver", "wild", "amber", "hidden", "misty", "stone", "winter", "green", "sunlit"];
const places = ["valley", "grove", "hearth", "ridge", "hollow", "meadow", "haven", "forest", "brook", "hill", "shore", "glade"];
export function randomServerName(): string {
  const values = crypto.getRandomValues(new Uint32Array(2));
  return `${adjectives[values[0] % adjectives.length]}-${places[values[1] % places.length]}`;
}
