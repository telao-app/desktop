package app.telao

import android.graphics.Color
import android.os.Bundle
import android.view.ViewGroup
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    // site sempre escuro: barras transparentes com ícones claros, também no tema claro do aparelho
    enableEdgeToEdge(
      statusBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
      navigationBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
    )
    super.onCreate(savedInstanceState)

    // O site é desenhado de ponta a ponta (edge-to-edge), mas o WebView do Android não repassa de forma confiável o
    // env(safe-area-inset-*) para o CSS. Então o recuo é aplicado aqui, por fora: barra de status, barra de
    // navegação (3 botões ou gestos, inclusive as que somem e aparecem), recorte da câmera (furo, notch) e teclado.
    // O fundo que aparece nessas faixas é o da janela (#16191A, igual ao site).
    val raiz = findViewById<ViewGroup>(android.R.id.content)
    ViewCompat.setOnApplyWindowInsetsListener(raiz) { v, insets ->
      val tipos = WindowInsetsCompat.Type.systemBars() or
        WindowInsetsCompat.Type.displayCutout() or
        WindowInsetsCompat.Type.ime()
      val i = insets.getInsets(tipos)
      v.setPadding(i.left, i.top, i.right, i.bottom)
      WindowInsetsCompat.CONSUMED
    }
    ViewCompat.requestApplyInsets(raiz)
  }
}
