// A tiny, safe arithmetic expression evaluator for the Calculator tool.
//
// No `eval`/`Function`: a hand-written tokenizer + recursive-descent parser so
// user input can never execute code. Supports + - * / , parentheses, unary
// +/-, a postfix `%` (percent = value / 100), decimals and scientific
// notation. Throws `CalcError` on malformed input.

export class CalcError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'CalcError';
  }
}

type Tok =
  | { t: 'num'; v: number }
  | { t: 'op'; v: '+' | '-' | '*' | '/' | '%' }
  | { t: 'lp' }
  | { t: 'rp' };

function tokenize(src: string): Tok[] {
  const toks: Tok[] = [];
  let i = 0;
  while (i < src.length) {
    const c = src[i];
    if (c === ' ' || c === '\t' || c === '\n' || c === ',') {
      i++;
      continue;
    }
    if (c === '(') {
      toks.push({ t: 'lp' });
      i++;
      continue;
    }
    if (c === ')') {
      toks.push({ t: 'rp' });
      i++;
      continue;
    }
    if (c === '+' || c === '-' || c === '*' || c === '/' || c === '%' || c === '×' || c === '÷') {
      const v = c === '×' ? '*' : c === '÷' ? '/' : (c as '+' | '-' | '*' | '/' | '%');
      toks.push({ t: 'op', v });
      i++;
      continue;
    }
    // number: digits, one dot, optional exponent
    const m = /^(\d+(\.\d+)?|\.\d+)([eE][+-]?\d+)?/.exec(src.slice(i));
    if (m) {
      toks.push({ t: 'num', v: Number(m[0]) });
      i += m[0].length;
      continue;
    }
    throw new CalcError(`Unexpected character "${c}"`);
  }
  return toks;
}

// Grammar:
//   expr   := term (("+" | "-") term)*
//   term   := factor (("*" | "/") factor)*
//   factor := postfix
//   postfix:= unary ("%")*
//   unary  := ("+" | "-") unary | primary
//   primary:= num | "(" expr ")"
class Parser {
  private pos = 0;
  constructor(private toks: Tok[]) {}

  private peek(): Tok | undefined {
    return this.toks[this.pos];
  }
  private next(): Tok | undefined {
    return this.toks[this.pos++];
  }

  parse(): number {
    const v = this.expr();
    if (this.pos !== this.toks.length) {
      throw new CalcError('Unexpected trailing input');
    }
    return v;
  }

  private expr(): number {
    let v = this.term();
    for (;;) {
      const p = this.peek();
      if (p && p.t === 'op' && (p.v === '+' || p.v === '-')) {
        this.next();
        const r = this.term();
        v = p.v === '+' ? v + r : v - r;
      } else {
        break;
      }
    }
    return v;
  }

  private term(): number {
    let v = this.postfix();
    for (;;) {
      const p = this.peek();
      if (p && p.t === 'op' && (p.v === '*' || p.v === '/')) {
        this.next();
        const r = this.postfix();
        if (p.v === '/') {
          if (r === 0) throw new CalcError('Division by zero');
          v = v / r;
        } else {
          v = v * r;
        }
      } else {
        break;
      }
    }
    return v;
  }

  private postfix(): number {
    let v = this.unary();
    for (;;) {
      const p = this.peek();
      if (p && p.t === 'op' && p.v === '%') {
        this.next();
        v = v / 100;
      } else {
        break;
      }
    }
    return v;
  }

  private unary(): number {
    const p = this.peek();
    if (p && p.t === 'op' && (p.v === '+' || p.v === '-')) {
      this.next();
      const r = this.unary();
      return p.v === '-' ? -r : r;
    }
    return this.primary();
  }

  private primary(): number {
    const tok = this.next();
    if (!tok) throw new CalcError('Unexpected end of input');
    if (tok.t === 'num') return tok.v;
    if (tok.t === 'lp') {
      const v = this.expr();
      const close = this.next();
      if (!close || close.t !== 'rp') throw new CalcError('Missing ")"');
      return v;
    }
    throw new CalcError('Expected a number or "("');
  }
}

/** Evaluate an arithmetic expression. Throws {@link CalcError} on bad input. */
export function evaluate(expr: string): number {
  const trimmed = expr.trim();
  if (trimmed === '') throw new CalcError('Empty expression');
  const result = new Parser(tokenize(trimmed)).parse();
  if (!Number.isFinite(result)) throw new CalcError('Result is not finite');
  return result;
}

/** Non-throwing variant for live UI feedback. */
export function tryEvaluate(expr: string): { ok: true; value: number } | { ok: false; error: string } {
  try {
    return { ok: true, value: evaluate(expr) };
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : String(e) };
  }
}
