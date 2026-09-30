Feature: OpenID Connect login yields a verified identity for custom handlers
  The provider authenticates; the application authorizes. A login produces
  only a verified issuer and subject, bound to a fresh server-side session.

  Scenario: A verified identity cannot be forged and ends at logout
    Then an OIDC login gives custom handlers a verified subject that no request can change, until logout revokes it
