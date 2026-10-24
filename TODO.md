# TODO

## Rediseño de la interfaz — hecho (0.4.0, 24-10-2026)
- Diseño completo en `docs/diseno/` (tableros de teléfono, horizontal, tablet y PC) y programado en
  `panels/phone_up.rs`: isla de herramientas, dock con tira y pistas, menú del dibujo, color flotante,
  inicio, exportar, tarjeta de gestos, tablet (vista estudio) y PC.
- Sonido dentro de la línea de tiempo: filas con forma de onda, mover/recortar clips, barra del clip
  (volumen, cortar, silenciar, borrar, fundidos, normalizar, separar en frases), grabar voz e importar
  desde la última fila.
- Android: importar con el selector de archivos del sistema (`android/TrazoPicker.java`).
- En prueba (`test/rediseno`): vista limpia en tablet (la UI del teléfono) que se alterna con la de estudio.

## Pendiente del rediseño
- [ ] Mantener el dedo quieto para tomar color y rechazo de palma con lápiz (el diseño lo prometía).
- [ ] Seleccionar varios dibujos («Seleccionar varios» del menú del dibujo).
- [ ] PC: pista de cámara con fotogramas clave, «solo» por pista, marcadores y tiradores del rango.
- [ ] Mover un clip de sonido a otra pista arrastrándolo en vertical.
- [ ] Capturas nuevas para el README.

## Idiomas — hecho (23-10-2026)
- Interfaz en español e inglés (`crates/trazo_app/src/i18n.rs`, tabla `i18n/en.rs`). Los textos se
  escriben en español con `tr("…")` / `tf!("… {}", x)`; se elige el idioma al abrir la app por primera
  vez (se propone el del sistema) y después desde el menú del proyecto. Se guarda en `idioma.txt`.
- Los proyectos guardan los nombres por defecto en español («Capa 2»); se traducen al mostrarlos.
- Pruebas: todo `tr("…")` tiene traducción, nombres del motor (pinceles, modos, formatos) también.
- Pendiente: decimales con coma en los deslizadores de egui; más idiomas (portugués, francés).

## Línea de tiempo por capas — hecha (23-10-2026)
- Una pista por capa; cada dibujo dura N fotogramas (`Frame.cont` marca las continuaciones).
- Estirar arrastrando el borde (burbuja ×N · s), −/+ en la barra de acciones, añadir, duplicar,
  mover y borrar dibujos por capa; deshacer de todo como «línea de tiempo».
- Papel cebolla por capa (dibujo anterior/siguiente de esa capa). Formato compatible: los
  proyectos viejos con «hold» se convierten al abrir.
- Pendiente: arrastrar un dibujo para moverlo, selección múltiple de dibujos, pista del clip de video.

## Fase 4: importar video (rotoscopia) — hecha (23-10-2026)
- Capa de video: fotogramas en JPEG (máx. 1280 px, 3600 imágenes, 400 MB), decodificados
  al vuelo con caché y precarga. Solo referencia por defecto (no se exporta), tinte gris/azul,
  inicio, recorte, escala, posición, ajustar/llenar, crear fotogramas a unos/doses/treses.
- Importar MP4/MOV/3GP/WebM/MKV en Android (MediaCodec, rotación de videos
  verticales, color BT.601/709 y rango completo, plan B con el decodificador por software)
  y GIF en todas las plataformas. Audio del tramo importado como clip.
- Guardado: `video/<uid>/…` en el `.anim`; el autoguardado copia sin recomprimir; un video
  dañado se conserva tal cual. Deshacer agrupa bien aunque el historial esté lleno.
- Probado en el emulador (importar a doses, tinte, llenar lienzo, deshacer, guardar/reabrir)
  y en un Samsung Galaxy A37 real (video vertical 1080×1920 con su decodificador por hardware).
- El decodificador escribe YUV en sus propios búferes (no ImageReader: en Samsung/Exynos daba
  memoria ilegible y cerraba la app); lecturas con límites comprobados.

## Pendiente (siguiente)
- [ ] Clip de video en la línea de tiempo: arrastrar para mover el inicio, bordes para recortar.
- [ ] Mover/escalar/rotar el video con tiradores en el lienzo (hoy: números y ajustar/llenar).
- [ ] El video avanza por tiempo real durante la reproducción aunque el dibujo dure 2+ tiempos
      (hoy cambia de imagen cuando cambia el dibujo).
- [ ] Papel cebolla del video (anterior/siguiente imagen) con su propio interruptor.
- [ ] Vincular el clip de audio con la capa de video (mover/recortar juntos).
- [ ] Cargar los fotogramas del video bajo demanda (hoy se leen todos al abrir el proyecto).
- [ ] Importar desde la galería con el selector de fotos de Android y grabar video con la cámara.
- [ ] Conversión YUV→RGB en enteros y directa al tamaño final (importar 4K más rápido).
- [ ] Decodificar solo el tramo de audio elegido.

## Más adelante
- MP4 en escritorio (importar y exportar) con Media Foundation.
- Exportar en segundo plano (servicio Android), marca de agua, hoja de sprites.
