# config/

Reservado para configuração estruturada futura (por exemplo, perfis de
risco por ambiente, arquivos de parâmetros por estratégia) quando um único
`.env` deixar de ser suficiente.

Hoje, toda a configuração de runtime é feita por variáveis de ambiente —
veja `.env.example` na raiz do repositório e `crates/app/src/config.rs`.
Este diretório intencionalmente ainda não possui nenhum código que dependa
dele; não adicione aqui arquivos que nada lê.
