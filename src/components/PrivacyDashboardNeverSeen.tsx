export function PrivacyDashboardNeverSeen() {
  return (
    <section aria-labelledby="pd-nunca-veo" className="privacy-dashboard__never-seen">
      <h4 id="pd-nunca-veo">Qué no veo nunca</h4>
      <ul>
        <li>La URL completa de los recursos que guardas — solo veo el dominio.</li>
        <li>El título de las páginas — se cifra y nunca se descifra para análisis.</li>
        <li>El contenido de las páginas — el sistema nunca lo lee.</li>
        <li>Tu identidad ni nada que pueda identificarte fuera de este dispositivo.</li>
      </ul>
      <p className="privacy-dashboard__never-seen-note">
        Todo lo anterior se almacena cifrado localmente con AES-256-GCM y nunca se transmite.
      </p>
    </section>
  );
}
