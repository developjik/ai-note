import { ui } from "../uiStrings";

export function ReviewScreen() {
  return (
    <section aria-label={ui.review.title}>
      <button>{ui.review.approve}</button>
      <button>{ui.review.reject}</button>
    </section>
  );
}
