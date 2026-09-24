# Frontend da UI de observabilidade

React + Vite + Tailwind + shadcn/ui + Animate UI (Sliding Number).

Somente leitura: consome as rotas `/api/*` do Axum em `crates/web`.
Nenhuma regra financeira é calculada aqui.

## Filosofia de UX (obrigatória)

Ao desenhar qualquer bloco:

1. Escrever 1–2 frases em português objetivo (“para leigo”).
2. Quebrar a frase em micro-peças (`InlineAsset`, `InlineStrategy`, `InlineMoney`, …).
3. Só então montar o componente.
4. Números/tabelas técnicas ficam atrás de “Ver detalhe” (accordion/sheet).

As frases puras (texto) vivem em `src/lib/narratives.ts`. A UI monta a mesma
ideia com micro-componentes em `src/components/narrative/`.

## Desenvolvimento

```bash
# terminal 1 — API
cargo run -p web

# terminal 2 — Vite com proxy para :58080
cd crates/web/frontend
npm install
npm run dev
```

## Build para o binário

```bash
cd crates/web/frontend
npm install
npm run build
```

Isso gera `crates/web/static/`, servido pelo próprio `quant-engine-web`.
