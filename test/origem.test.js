// Conferência de origem do IPC (L-E10-02). Rode: npm test.
const assert = require('node:assert/strict');
const { test } = require('node:test');
const { origemDe, protegido, remetentePermitido } = require('../origem.js');

const BASE = 'https://app.telao-app.com.br';
const de = (url) => ({ senderFrame: url === undefined ? null : { url } });

test('aceita só a origem do Telão (qualquer caminho, query ou hash)', () => {
  assert.equal(remetentePermitido(de('https://app.telao-app.com.br/app'), BASE), true);
  assert.equal(remetentePermitido(de('https://app.telao-app.com.br/app/x?y=1#z'), BASE), true);
  assert.equal(remetentePermitido(de('https://app.telao-app.com.br:443/'), BASE), true);
});

test('recusa outra origem, subdomínio parecido, http, outra porta, file:, about:, data: e quadro ausente', () => {
  for (const url of [
    'https://evil.example/', 'https://app.telao-app.com.br.evil.example/', 'https://evil.example/https://app.telao-app.com.br',
    'http://app.telao-app.com.br/', 'https://app.telao-app.com.br:8443/', 'https://telao-app.com.br/', 'https://challenges.cloudflare.com/',
    'file:///etc/passwd', 'about:blank', 'data:text/html,oi', '', 'não é url',
  ]) assert.equal(remetentePermitido(de(url), BASE), false, url);
  assert.equal(remetentePermitido(de(undefined), BASE), false);
  assert.equal(remetentePermitido({}, BASE), false);
  assert.equal(remetentePermitido(null, BASE), false);
});

test('origem local de desenvolvimento (TELAO_URL) vale do mesmo jeito', () => {
  assert.equal(remetentePermitido(de('http://localhost:5173/app'), 'http://localhost:5173'), true);
  assert.equal(remetentePermitido(de('http://localhost:5174/app'), 'http://localhost:5173'), false);
  assert.equal(origemDe('file:///x'), null);
});

test('protegido: chama o comando da origem certa; recusa e registra a outra (handle rejeita, on ignora)', () => {
  const logs = [];
  let chamadas = 0;
  const fn = (_e, x) => { chamadas++; return x * 2; };
  const h = protegido(BASE, 'teclas', fn, { log: (m) => logs.push(m) });
  assert.equal(h(de('https://app.telao-app.com.br/app'), 21), 42);
  assert.throws(() => h(de('https://evil.example/'), 1), /origem não permitida/);
  const o = protegido(BASE, 'som-janela', fn, { rejeitar: false, log: (m) => logs.push(m) });
  assert.equal(o(de('https://evil.example/'), 1), undefined);
  assert.equal(chamadas, 1);
  assert.equal(logs.length, 2);
  assert.match(logs[0], /IPC recusado: "teclas" de https:\/\/evil\.example\//);
});
