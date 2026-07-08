// Простой subsequence-fuzzy matcher (как в VS Code Quick Open, без внешней
// зависимости): каждая буква запроса обязана встретиться в тексте по
// порядку, но не обязательно подряд. Идёт вместе с очень дешёвым скорингом
// (плотнее совпадение = выше счёт) — этого достаточно для сотен, не
// десятков тысяч, команд в палитре (см. NFR-1: <16ms на 500+ команд).

export interface FuzzyMatchResult {
  matched: boolean;
  score: number;
}

export function fuzzyMatch(query: string, target: string): FuzzyMatchResult {
  if (query.length === 0) {
    return { matched: true, score: 0 };
  }

  const q = query.toLowerCase();
  const t = target.toLowerCase();

  let qi = 0;
  let score = 0;
  let lastMatchIndex = -1;

  for (let ti = 0; ti < t.length && qi < q.length; ti += 1) {
    if (t[ti] === q[qi]) {
      // Совпадения подряд (без разрывов) стоят дороже — это то, что отличает
      // "хороший" fuzzy-скоринг от простого subsequence-теста.
      const gap = lastMatchIndex === -1 ? 0 : ti - lastMatchIndex - 1;
      score += 10 - Math.min(gap, 9);
      if (ti === 0 || t[ti - 1] === " ") {
        score += 5; // бонус за совпадение с началом слова
      }
      lastMatchIndex = ti;
      qi += 1;
    }
  }

  return { matched: qi === q.length, score };
}
