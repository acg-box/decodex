import { marked } from "marked";

marked.setOptions({
  gfm: true,
  breaks: false,
});

export function renderMarkdownInline(markdown: string): string {
  return marked.parseInline(markdown) as string;
}
