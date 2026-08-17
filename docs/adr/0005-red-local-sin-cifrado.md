# ADR 0005 · Acceso desde la red local sin cifrado

- **Estado**: aceptada
- **Fecha**: 2026-08-17
- **Sustituye**: el punto 2 de la decisión del [ADR 0003](0003-acceso-desde-la-red-local.md)
  («transporte cifrado obligatorio») y su riesgo aceptado 1
- **Modifica**: la invariante 9 de [`CLAUDE.md`](../../CLAUDE.md)

## Contexto

El ADR 0003 exigía TLS para todo el tráfico que saliera de loopback, con un
certificado autofirmado que Nexo genera y que cada cliente acepta a mano. Se
implementó así y se usó. Lo que apareció al usarlo:

1. **El certificado quedó atado a la IP de la máquina.** Los nombres que
   llevaba eran `127.0.0.1`, `::1`, `localhost` y **una** IP: la de la
   interfaz por la que sale la ruta por defecto. En un portátil eso cambia al
   cambiar de red, al renovarse el DHCP o al conectar una VPN. Cuando cambia,
   el certificado deja de ser válido para la dirección que el panel anuncia y
   el cliente lo rechaza. El [PR #18](https://github.com/miglesiassarria/Nexo/pull/18)
   añadió detectarlo y rehacer el certificado, lo que resuelve el rechazo pero
   a costa de una huella nueva que hay que volver a aceptar en cada cliente
   cada vez que el portátil cambia de red.
2. **Las direcciones de las demás interfaces nunca estuvieron cubiertas.** Un
   cliente que llegara por una interfaz que no fuera la de la ruta por defecto
   —red mallada, bridge de una máquina virtual, VPN— era rechazado siempre.
3. **La fricción cae entera del lado del cliente y se repite.** «Aceptar el
   certificado una vez por dispositivo» solo es una vez si la identidad del
   servidor es estable, y no lo era.

Se consideró y se descartó la alternativa que arreglaba esto conservando el
cifrado (ver más abajo). El usuario, informado del coste, decidió que para su
caso de uso —sus propios equipos, en su propia red, consumiendo modelos— el
cifrado no compensa la fricción.

## Decisión

**El acceso desde la red local se sirve en HTTP plano, sin certificado.**

1. **Se elimina TLS del gateway y con él todo el andamiaje del certificado**:
   el módulo `tls_cert`, `gateway::serve_on_tls`, el fichero de constancia de
   direcciones y las dependencias `rcgen` y `axum-server`. No queda una opción
   apagada: queda menos código. Un flujo que no existe no puede fallar ni
   pedir mantenimiento.
2. **Un único listener.** Con `allow_lan` activo, `0.0.0.0:<puerto>` en HTTP
   plano atiende tanto a la red como a loopback. Desaparece el par de
   listeners que el PR #18 tuvo que montar para que activar el modo red no
   dejara sin servicio a los clientes locales: sin TLS, el problema que
   resolvían no existe.
3. **Las otras dos piezas siguen siendo obligatorias, sin cambios.** Token de
   aplicación por Bearer en cada petición, y límites por aplicación en las
   vías de suscripción (invariante 4, mitigación del ADR 0001). Esta decisión
   quita una de las tres capas, no las tres.
4. **El aviso previo dice la verdad de lo que pasa.** Activar el modo red
   sigue exigiendo aceptar un aviso explícito, y ese aviso dice ahora, con esas
   palabras, que el tráfico **no va cifrado** y qué implica. La mitigación de
   quitar el cifrado es que el usuario sepa exactamente qué acepta, no un
   texto genérico.
5. **El panel enumera todas las direcciones por las que queda expuesto**, no
   solo la de la ruta por defecto. Era la mitigación del riesgo 3 del ADR
   0003, nunca implementada; al retirar el cifrado pasa a ser la principal que
   queda, así que se implementa de verdad.
6. **Sigue desactivado por defecto.** Nexo escucha solo en `127.0.0.1` salvo
   activación explícita. Eso no se toca.

Nada de esto habilita acceso desde fuera de la red local. Sin cifrado, la
exclusión es además más terminante que antes: exponer esto a Internet sería
regalar el token en claro por el camino.

## Alternativa descartada

**Certificado atado a un nombre estable (`<equipo>.local`) en lugar de a la
IP.** Es la solución técnicamente correcta al problema del contexto: mDNS
—Bonjour, presente en macOS sin instalar nada— resuelve ese nombre desde
cualquier equipo de la red, sobrevive a los cambios de dirección, y el
certificado no caduca hasta el año 4096, así que aceptarlo sería de verdad una
vez por dispositivo y para siempre. Se comprobó que el nombre resuelve en la
máquina del usuario.

Se descartó por decisión explícita del usuario tras exponerle esta opción como
la recomendada y detallarle el coste de la otra. Sus razones: son sus propios
equipos en su propia red, y ningún cliente debería tener que aceptar nada para
consumir modelos. Se registra aquí que la alternativa existía, funcionaba y era
la recomendada, para que quien lea esto dentro de un año no crea que se retiró
el cifrado por no saber cómo mantenerlo.

Si la decisión se revierte, esta es la vía por la que hay que volver.

## Riesgos aceptados

### 1. El token de aplicación viaja en claro por la red

Es el riesgo principal, y no es abstracto: el token va en la cabecera
`Authorization` de **cada** petición. Quien lo capture obtiene acceso a los
modelos del usuario con sus permisos y sus límites, es decir, a su suscripción
y a sus API keys de pago.

Qué protege y qué no:

- El enlace inalámbrico de una red WPA2/WPA3 ya va cifrado, así que un
  observador pasivo desde fuera de la red no lo lee.
- Dentro de la misma red **sí** es alcanzable: cualquier equipo conectado
  puede intentar un ataque de intermediario (suplantar el router por ARP, por
  ejemplo) y leer todo el tráfico. No requiere medios extraordinarios.
- En una red que el usuario no controla —oficina, hotel, invitados en casa—
  hay que dar por hecho que es leíble.

**Mitigación.** El aviso previo lo dice con estas palabras antes de activarlo.
El token es revocable en cualquier momento desde Aplicaciones, y revocar corta
el acceso al instante. El interruptor es reversible: desactivar el modo red
devuelve el gateway a loopback, donde el tráfico no sale de la máquina.
Recomendación registrada: desactivarlo antes de conectarse a una red ajena.

### 2. El contenido de las conversaciones viaja en claro

Lo mismo se aplica a los mensajes y a las respuestas. Nexo no los guarda por
defecto (invariante 10), pero por el cable pasan legibles.

**Mitigación.** La misma: el aviso lo dice, y el modo es reversible. Quien
necesite confidencialidad en tránsito no debe usar el modo red.

### 3. Cualquier equipo de la red puede llamar a la puerta

Sin cambios respecto al ADR 0003: escuchar en `0.0.0.0` expone el gateway a
toda la red. Sigue haciendo falta el token, así que llamar no es entrar.

**Mitigación.** Desactivado por defecto, aviso explícito, y el panel enumera
todas las direcciones por las que se está escuchando para que el usuario vea
por dónde queda expuesto.

### 4. La clave privada del certificado anterior sigue en disco

Los certificados ya generados quedan en `<datos>/tls/`. No se borran de forma
automática: son ficheros del usuario y borrar claves privadas por su cuenta no
es cosa de un arranque. Ya no se usan para nada.

**Mitigación.** Se documenta que se pueden borrar a mano y se dice cómo.

## Consecuencias arquitectónicas

1. **`tls_cert` desaparece.** Con él, la única pieza que el invariante 7
   («lo frágil vive aislado») señalaba en este camino.
2. **`GatewayBindPlan` vuelve a ser una dirección y nada más.** Sin
   certificado y sin listener de loopback aparte.
3. **`LanAccessInfo` deja de hablar de huellas y de ficheros de certificado**
   y pasa a llevar la lista de direcciones de escucha y el puerto.
4. **`net` gana la enumeración de interfaces** (`if-addrs`), que sustituye a
   «la IP de la ruta por defecto» como fuente de lo que el panel muestra.
5. **Dos dependencias fuera, una dentro**: se retiran `rcgen` y `axum-server`,
   se añade `if-addrs`.
6. **La invariante 9 de `CLAUDE.md` se reescribe** para decir que el modo red
   exige autenticación y autorización, que el transporte va sin cifrar, y que
   eso se acepta de forma explícita y consciente por esta decisión.

## Revisión

Esta decisión debe revisarse si:

- El usuario pasa a usar el modo red en una red que no controla, o con datos
  que no quiere que sean legibles en tránsito. Entonces se vuelve por la
  alternativa descartada: certificado atado a `<equipo>.local`.
- Aparece la necesidad de acceso desde fuera de la red local: eso sigue siendo
  un ADR distinto, y sin cifrado no es discutible siquiera.
- Un cliente relevante deja de admitir HTTP plano contra un destino que no sea
  `localhost`. Hoy no es el caso, pero es la clase de cambio que llega desde
  fuera sin avisar.
